# E4B. Relecture independante : fournisseurs, fichiers et pages

Relecture du code commite (74bdcd1 pour le lot 1, 2028cdd~1..5260b7a pour les commits
precedents). Perimetre : dialogue avec les fournisseurs de modeles (openai_compat.rs, error.rs,
routing.rs, main.rs), lecture et recherche de fichiers (file_ops.rs), outil ViewPage
(page_view.rs, web_page.rs, permissions.rs). Les numeros de ligne sont ceux du commit 74bdcd1.

## E4B-01 Schemas reduits : les proprietes renommees ne sont jamais remises a leur nom (grep_search, outils MCP)
- Gravite : moyenne
- Ou : rust/crates/api/src/providers/openai_compat.rs:344 (`plain_property_name`) et :364-377
  (`plain_schema`) ; rust/crates/runtime/src/file_ops.rs:114-124 (`GrepSearchInput`)
- Scenario : un serveur refuse un mot-cle des schemas d'outils (par exemple l'API Gemini avec
  `Unknown name "additionalProperties"`, cas vise par le test du lot 1). `send_negotiated` retient
  `remember_plain_schemas(base_url)` et toutes les requetes suivantes vers ce serveur passent par
  `plain_tool_schemas`, qui renomme toute propriete dont le nom ne commence pas par une lettre
  ou `_` (les premiers caracteres sont retires).
  (1) Un serveur MCP branche sur une API OData (Microsoft Graph, par exemple) declare
  `$filter` et `$top` : le modele ne voit que `filter` et `top`, appelle l'outil avec ces cles,
  et `call_mcp_tool` les transmet telles quelles ; le serveur MCP ne les connait pas, la requete
  part sans filtre ou echoue. Deux proprietes `@type` et `type` deviennent la meme cle :
  `renamed` (lignes 364-370) n'en garde qu'une. (2) grep_search n'annonce plus `-i`, `-A`,
  `-B`, `-C`, `-n` mais `i`, `A`, `B`, `C`, `n` ; `{"pattern":"todo","i":true}` est lu par
  `GrepSearchInput`, qui ne connait que `-i` (serde `rename` sans `alias`, sans
  `deny_unknown_fields`) : la cle est jetee sans erreur, la recherche reste sensible a la casse,
  et le modele conclut qu'il n'y a pas de `TODO`. La memoire est par serveur (`base_url`) : chez
  OpenRouter, un seul modele dont le fournisseur refuse un schema fait passer tous les modeles
  OpenRouter de la session en schemas reduits.
- Preuve : le commentaire de `plain_property_name` affirme « `-A` devient `A` (les outils de
  Claw acceptent ces noms) », mais `git grep` au commit 74bdcd1 ne trouve aucun `alias = "A"` ni
  aucune conversion retour (`plain_property_name` n'est appele que dans `plain_schema`), et
  `input_check` ne verifie que les arguments obligatoires. Le cas de grep_search est deja la
  tache A10 du suivi (E3-02) ; celui des outils MCP et des noms confondus ne l'est pas, et des
  `alias` dans `GrepSearchInput` ne le regleraient pas.
- Remede : garder, pour chaque outil, la table des noms renommes par `plain_schema` et remettre
  les noms d'origine dans les arguments des appels recus (flux et reponse entiere) quand
  `wants_plain_schemas` est vrai ; ne pas renommer quand deux noms donneraient la meme cle. Pour
  les outils de Claw, ajouter aussi `alias` aux champs de `GrepSearchInput`
  (`#[serde(rename = "-A", alias = "A")]`, idem `B`, `C`, `n`, `i`). Test : grep_search execute
  avec `{"i": true}` et `{"A": 1}`.
- Etat : corrigé (lot 2b) : `restore_property_names` rend leurs vrais noms aux arguments reçus (agent et sous-agents), `plain_schema` ne réduit plus un nom vers un nom déjà pris, `grep_search` accepte aussi `A`, `B`, `C`, `n`, `i` (A10).

## E4B-02 L'aide d'erreur lit « 429 » ou « 401 » dans n'importe quel nombre du message
- Gravite : basse
- Ou : rust/crates/claw-cli/src/main.rs:205 et :211, `error_hint` (meme aide dans web.rs:807 et
  :903)
- Scenario : la conversation depasse le contexte d'un modele OpenRouter et `auto_compact(true)` ne
  suffit pas (gros resultat d'outil dans le tour, ou `claw -p` avec un long texte colle).
  OpenRouter repond 400 avec `{"error":{"message":"This endpoint's maximum context length is
  131072 tokens. However, you requested about 140129 tokens (140129 of text input)...",
  "code":400}}`. Ce corps n'a pas de champ `type`, donc `ApiError::Api` s'affiche avec le corps
  entier : « l'api a retourné 400 Bad Request : {...140129...} ». `error_hint` teste d'abord
  `lower.contains("429")`, puis `lower.contains("401")` : « 140129 » contient « 401 ».
  L'utilisateur lit « verifiez la cle OPENROUTER_API_KEY » au lieu de « resumez-la avec
  /compact ». Avec « 142900 tokens », il lit « limite de requetes atteinte... reessayez plus
  tard ». Meme effet pour un identifiant ou un horodatage du corps qui contient ces chiffres.
- Preuve : `returned_status(problem)` est deja calcule ligne 204, mais les tests de la ligne 205
  (`lower.contains("429")`) et 211 (`lower.contains("401")`) cherchent les chiffres partout dans
  le texte, avant le test du contexte (lignes 221-226). Le test
  `error_hints_cover_free_model_limits` n'a aucun message d'erreur qui porte un nombre.
- Remede : remplacer `lower.contains("429")` par `status == Some(429)` (en gardant
  `contains("rate limit")`) et `lower.contains("401")` par `status == Some(401)` ; placer le
  test du contexte (413, « context length »...) avant les tests de quota et de cle. Ajouter un
  cas de test avec « you requested about 140129 tokens ».
- Etat : corrigé (lot 2b) : `error_hint` lit le code HTTP (`returned_status`) pour 429 et 401.

## E4B-03 grep_search ne trouve jamais un motif sur plusieurs lignes, meme avec multiline
- Gravite : moyenne
- Ou : rust/crates/runtime/src/file_ops.rs:876 et :920-923, `grep_search`
- Scenario : le modele (habitue a l'outil Grep de Claude Code, ou `multiline` veut dire « le
  motif peut couvrir plusieurs lignes ») cherche ou une fonction appelle `unwrap` juste apres sa
  signature : `{"pattern":"fn charger\\(.*\\)\\s*\\{\\s*\\n\\s*let .*unwrap","multiline":true,
  "output_mode":"content"}`. Le motif est compile avec `dot_matches_new_line(true)`, mais en mode
  `content` et `files_with_matches` (le mode par defaut) le texte est d'abord coupe en lignes
  (`file_contents.lines()`) et chaque ligne est testee seule (`regex.is_match(lines[index])`).
  Un motif qui contient `\n`, ou `\s*` qui doit franchir une fin de ligne, ne peut correspondre a
  aucune ligne : le resultat est vide, sans note. Le modele conclut que ce code n'existe pas. En
  mode `count`, la meme recherche trouve les occurrences (`regex.find_iter(&file_contents)` sur le
  texte entier) : deux reponses contradictoires pour la meme question.
- Preuve : `multiline` n'est lu qu'a la ligne 876 ; les lignes 920-923 decoupent toujours en
  lignes. Aucun test de file_ops ne passe `multiline: Some(true)` (lignes 1769, 1800, 1830, 1883,
  1941 : `Some(false)` ou `None`).
- Remede : quand `multiline` est vrai, chercher sur le texte entier (`regex.find_iter`), puis
  convertir chaque debut et fin de correspondance en numeros de ligne (compter les `\n` avant
  l'octet) pour `files_with_matches` et `content` (afficher toutes les lignes couvertes, avec le
  contexte). Ajouter un test avec un motif `a\\n\\s*b` sur deux lignes, dans les trois modes.
- Etat : a verifier

## E4B-04 write_file retire la marque UTF-8 (BOM) et les fins de ligne CRLF d'un fichier existant
- Gravite : moyenne
- Ou : rust/crates/runtime/src/file_ops.rs:224-243 (`decode_text`), :404-434 (`write_file`)
- Scenario : sous Windows, `deploy.ps1` est enregistre par VS Code ou l'ISE en « UTF-8 avec
  BOM » (octets EF BB BF), en CRLF, et contient `Write-Host "Opération terminée"`. Le modele
  le lit (read_file montre le texte ; le U+FEFF de tete est invisible), puis le reecrit en
  entier avec write_file, sans U+FEFF et avec des fins de ligne LF, comme le font les modeles.
  `decode_text` a classe le fichier `Utf8` (le BOM n'est pas un codage a part, il reste un
  caractere du texte), donc `encode_text(content, Utf8)` ecrit le texte du modele tel quel : le BOM disparait et le fichier passe en LF, sans `encoding_note`. Windows PowerShell 5.1
  lit un script sans BOM dans le code ANSI (Windows-1252) : il affiche chaque « é » comme deux signes (un A tilde puis le signe
  copyright),
  et une comparaison de chaine ou un chemin accentue du script ne correspond plus. Meme chose pour
  un CSV « UTF-8 » d'Excel (qui a besoin du BOM pour ne pas afficher ces deux signes). Pour un `.bat`, le
  passage en LF expose le defaut connu de cmd.exe sur `goto`/`call :etiquette`.
- Preuve : `decode_text` ne teste que les BOM UTF-16 (ligne 225) ; `std::str::from_utf8` garde
  le U+FEFF dans le texte (ligne 241-242). `write_file` ne garde que l'enumeration
  `TextEncoding` (ligne 412-414) et n'a aucun traitement des fins de ligne (contrairement a
  `apply_edit`, lignes 571-589, qui conserve le CRLF pour edit_file). La note « Written in the
  file's encoding » n'est emise que pour Windows-1252 et UTF-16 (lignes 419-423).
- Remede : dans `write_file`, quand le fichier existant commence par U+FEFF et que `content` ne
  le contient pas, le remettre en tete ; quand le fichier existant est en CRLF (toutes ses fins
  de ligne, ou une majorite) et que `content` n'a que des LF, convertir `\n` en `\r\n` avant
  l'encodage ; dire au modele dans `encoding_note` ce qui a ete garde. Ajouter un test (fichier
  BOM + CRLF reecrit par write_file : memes trois premiers octets, aucune ligne en LF seul).
- Etat : a verifier

## E4B-05 Les regles de permission ecrites pour WebFetch ne couvrent pas ViewPage
- Gravite : moyenne
- Ou : rust/crates/runtime/src/permissions.rs:102-119 (`covers_tool`), :396 (`from_input`)
- Scenario : l'utilisateur a mis dans ses reglages `"permissions": {"ask": ["WebFetch"]}` (forme
  prevue, voir le test de config.rs:1586) ou `"deny": ["WebFetch(domain:intranet.local)"]` pour
  que Claw ne lise pas le web, ou pas ce domaine, sans lui. Depuis 5260b7a, ViewPage est propose
  au modele des qu'Edge est installe (toujours le cas sous Windows). Le modele appelle
  `ViewPage {"url":"http://intranet.local/rapport"}`. `authorize` cherche une regle `deny` puis
  `ask` qui couvre l'outil : `covers_tool` compare « WebFetch » a « ViewPage »
  (`self.tool.eq_ignore_ascii_case(tool_name)`, ligne 113) et rend faux. ViewPage est en
  `ReadOnly`, donc `Allow` sans question : la page est chargee dans un vrai navigateur, son
  JavaScript s'execute, et son texte va au modele. La regle de l'utilisateur est contournee sans
  qu'il le sache (la page chargee peut aussi porter des donnees dans son adresse, comme avec
  WebFetch).
- Preuve : la ligne 396 donne bien a ViewPage une cible `Url`, mais `covers_tool` (lignes
  103-117) n'a pas d'entree qui relie WebFetch et ViewPage ; aucune autre regle par defaut ne
  vise ViewPage (`git grep '"ViewPage"'` au commit 74bdcd1 : outils, transcription, acp.rs).
- Remede : dans `covers_tool`, faire couvrir par une regle « WebFetch » les deux outils
  (`"webfetch" => &["WebFetch", "ViewPage"]`, comme « Read » couvre la recherche) ; dans
  `suggest_rule`, ecrire le nom de l'outil appele. Ajouter un test : `ask: ["WebFetch"]` fait
  demander ViewPage ; `deny: ["WebFetch(domain:x)"]` refuse `ViewPage http://x/`.
- Etat : corrigé (lot 2b) : une règle `WebFetch` couvre aussi `ViewPage` (`covers_tool`).

## E4B-06 ViewPage ne rend que le contenu de `<main>` : bandeaux d'erreur, en-tete et menus disparaissent
- Gravite : moyenne
- Ou : rust/crates/tools/src/web_page.rs:116-119 (`rendered_text`) et :122-149 (`read_page`),
  :28-30 (`HIDDEN_ELEMENTS`, `CHROME_ELEMENTS`) ; appel dans rust/crates/tools/src/page_view.rs:248
- Scenario : l'utilisateur signale « un message rouge s'affiche en haut et mon nom n'apparait
  pas ». La page rendue par le navigateur est
  `<body><header>Bonjour undefined</header><div class="toast erreur">Impossible de charger le
  panier (500)</div><main>... liste de produits, plus de 200 caracteres ...</main><footer>...
  </footer></body>`. Le modele appelle ViewPage, dont la description promet « what a user would
  see ». `rendered_text` reprend la lecture d'article de WebFetch : `main_content` trouve
  `<main>`, son texte depasse 200 octets (ligne 135), donc seul ce texte est rendu. « Bonjour
  undefined » et le bandeau d'erreur, hors de `<main>`, ne sont pas dans `text`. Le modele
  repond que la page affiche bien la liste et ne voit aucun des deux defauts signales. Meme
  perte pour `<nav>` et `<footer>` quand il n'y a pas de `<main>` (`drop_chrome` vrai d'abord),
  pour les choix d'un `<select>` et le contenu d'un `<textarea>` (ecartes lignes 26-28).
- Preuve : `rendered_text` appelle `read_page(html, "text/html", url, false)` sans option propre
  a ViewPage ; `read_page` garde `main` des que `text.len() >= 200` (ligne 135) et n'essaie la
  page entiere que si ce texte est trop court (lignes 136-143). Le test reel de ViewPage
  (tools/src/lib.rs, `view_page_shows_what_the_browser_displays_and_its_console`) n'a ni
  `<main>`, ni `<nav>`, ni `<footer>`.
- Remede : donner a ViewPage une lecture de la page entiere : `html_to_markdown(body, "", false)`
  sans passer par `main_content`, sans retirer `nav`/`footer`, et en gardant le texte des
  `option` et des `textarea`. Garder la coupe a 6 000 caracteres de page_view.rs. Ajouter au
  test reel un bandeau hors de `<main>` qui doit apparaitre dans `text`.
- Etat : corrigé (lot 2b) : `rendered_text` lit toute la page, en-tête, menus et pied compris. Restent les choix d'un `<select>` et le texte d'un `<textarea>`.

## E4B-07 glob_search avec `path` sur un lecteur reseau ne rend jamais aucun fichier
- Gravite : moyenne
- Ou : rust/crates/runtime/src/file_ops.rs:783-795 (`glob_search`), :1279-1285 (`display_path`),
  :1408-1412 (`normalize_path`)
- Scenario : le projet est sur un lecteur reseau `Z:` (dossier partage d'un NAS, poste
  d'entreprise) ou ouvert par `\\serveur\partage\projet`. Le modele appelle
  `glob_search {"pattern":"**/*.py","path":"src"}`. `normalize_path` passe par `canonicalize`,
  qui rend sous Windows `\\?\UNC\serveur\partage\projet\src` pour un lecteur reseau.
  `display_path` retire `\\?\` seulement devant une lettre de lecteur et garde volontairement
  `\\?\UNC\...` (test lignes 1474-1476). Le motif `\\?\UNC\serveur\partage\projet\src\**\*.py`
  part tel quel a `glob::glob` : la caisse glob 0.3.3 (Cargo.lock) rend un iterateur vide, sans
  erreur, pour tout prefixe verbatim autre qu'un disque (`check_windows_verbatim`). Resultat :
  `numFiles: 0`, sans note. Le modele conclut que le dossier ne contient aucun fichier Python.
  Sans `path`, la base est `current_dir()` (non canonicalisee) et la recherche marche : le
  defaut n'apparait que quand le modele precise un dossier, ce qu'il fait souvent.
- Preuve : file_ops.rs ligne 784-791 (`normalize_path` puis `display_path(&base_dir.join(...))`) ;
  glob-0.3.3/src/lib.rs lignes 183-195 (`check_windows_verbatim` : seul `VerbatimDisk` est
  admis) et 233-242 (iterateur vide). grep_search, qui parcourt avec WalkDir, accepte ces
  chemins. Les crochets dans le dossier de base, autre cause du meme symptome, sont deja dans
  E1-10 (point 1).
- Remede : avant de construire le motif, ecrire `\\?\UNC\serveur\partage\...` sous la forme
  `\\serveur\partage\...` (que glob accepte), et echapper la base avec
  `glob::Pattern::escape` (E1-10). Mieux : parcourir la base avec WalkDir et comparer le chemin
  relatif au motif (remede de E2-12), ce qui supprime les deux causes. Test : base
  `\\?\UNC\...` simulee par une fonction de conversion testee a part.
- Etat : a verifier

## E4B-08 ViewPage annonce une console sans erreur quand il n'a pas pu lire le journal
- Gravite : basse
- Ou : rust/crates/tools/src/page_view.rs:211 (`render`) et :241-293 (`view_page`)
- Scenario : sous Ubuntu 22.04 ou 24.04, le seul navigateur est Chromium installe par snap
  (`/snap/bin/chromium`, trouve par `find_browser` via le nom `chromium`). Claw cree
  `/tmp/claw-page-<pid>-<nanos>/` et passe au navigateur `--user-data-dir` et
  `--log-file=/tmp/claw-page-.../console.log`. Un snap a son propre `/tmp` prive : le journal
  est ecrit dans `/tmp/snap-private-tmp/snap.chromium/tmp/claw-page-.../console.log`, que Claw
  ne voit pas. Le DOM, lui, arrive par la sortie standard (descripteur herite). Ligne 211,
  `std::fs::read(&log_path).unwrap_or_default()` rend un texte vide sans le dire ; le resultat
  porte `"console": []`, `"consoleErrors": 0` et une note qui affirme « what it wrote to the
  console ». La page de l'utilisateur leve une exception (`Uncaught TypeError`) ; le modele lit
  zero erreur et cherche ailleurs. Meme effet avec tout navigateur qui n'ecrit pas le journal la
  ou on le demande (`CLAW_BROWSER` vers un autre binaire Chromium, par exemple).
- Preuve : la lecture du journal ne distingue pas « fichier absent » de « journal sans message »
  (ligne 211) ; `view_page` n'a aucune note pour ce cas (lignes 266-285). Le profil reste en
  plus dans le `/tmp` prive du snap a chaque appel (le nettoyage ligne 218 ne voit que le
  dossier de l'hote).
- Remede : garder l'erreur de lecture du journal ; quand il est absent (ou vide alors que le
  navigateur a bien rendu une page), rendre `"consoleErrors": null` et ajouter la note « the
  browser's console log could not be read: console errors are unknown, not absent ». Dans
  `find_browser`, preferer un navigateur hors de `/snap/` quand il y en a un.
- Etat : corrigé (lot 2b) : journal absent ou vide, `consoleErrors` vaut `null` et une note dit que les erreurs sont inconnues ; un navigateur hors de `/snap/` est préféré.

## E4B-09 ViewPage accuse le serveur quand c'est le navigateur qui refuse de demarrer
- Gravite : basse
- Ou : rust/crates/tools/src/page_view.rs:191-209 (`render`), :162-180 (`wait_for`), :242-246
  (`view_page`)
- Scenario : Claw tourne sous Linux dans un conteneur en root (image Docker de developpement,
  conteneur de CI) avec Chromium installe. Le modele demarre le serveur de l'application puis
  appelle `ViewPage {"url":"http://localhost:3000/"}`. Chromium refuse de tourner en root sans
  `--no-sandbox` (« Running as root without --no-sandbox is not supported ») et s'arrete aussitot
  avec un code d'erreur. Sa sortie d'erreur part dans `Stdio::null()` (ligne 206), `wait_for`
  rend `Ok(())` quel que soit le code de sortie (ligne 166), le DOM est vide, et `view_page`
  repond « the browser returned no page for http://localhost:3000/: is the server running and
  the address right? (a connection that is refused gives an empty page) ». Le serveur marche ;
  le modele le relance, change de port, reessaie ViewPage, et brule des requetes gratuites sur
  une fausse piste. Meme message trompeur sous Windows si Edge plante ou est bloque par une
  strategie de l'entreprise.
- Preuve : le statut rendu par `child.try_wait()` n'est jamais lu (ligne 166 : `Ok(Some(_))`) ;
  `stderr(Stdio::null())` ligne 206 ; le seul diagnostic d'un DOM vide est la phrase sur le
  serveur (lignes 242-246).
- Remede : garder le code de sortie et la fin de la sortie d'erreur du navigateur (stderr vers
  un fichier du profil temporaire) ; DOM vide avec un code non nul : rendre « the browser
  (chemin) exited with code N before showing the page: <dernieres lignes> », sans parler du
  serveur. Pour le cas root, le dire tel quel (« le navigateur refuse de tourner en root ») ;
  ajouter `--no-sandbox` seulement si l'utilisateur l'accepte, puisque ViewPage ouvre aussi
  des pages hors du projet.
- Etat : a verifier

## E4B-10 Un serveur vLLM sans appel d'outils automatique fait echouer chaque tour au lieu du mode texte
- Gravite : basse
- Ou : rust/crates/api/src/providers/openai_compat.rs:620-654 (`send_negotiated`) ;
  rust/crates/api/src/providers/text_tools.rs:48-63 (`is_tools_unsupported_error`)
- Scenario : l'utilisateur sert un modele gratuit en local avec `vllm serve Qwen/...` (sans
  `--enable-auto-tool-choice --tool-call-parser`) et le declare comme fournisseur compatible.
  Claw envoie les outils avec `"tool_choice": "auto"` (main.rs:5940). vLLM repond 400 :
  `"auto" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set`.
  Dans la boucle de `send_negotiated` : pas `stream_options` ; `names_field(text,
  "tool_choice")` est faux (le message ecrit « tool choice » avec une espace, et
  « auto-tool-choice » avec des tirets) ; aucun mot de `SCHEMA_REFUSAL_MARKS` ; la boucle rend
  l'erreur. `is_tools_unsupported_error` ne connait que les messages d'Ollama, d'OpenRouter et
  de llama.cpp (`requires --jinja`) : pas de bascule en mode texte. Chaque tour s'arrete sur
  « l'api a retourné 400 » avec l'aide generique « essayez un autre modele », alors que le mode
  texte (`"toolMode": "text"`) fonctionnerait. Retirer seulement `tool_choice` ne suffirait pas :
  vLLM remet `auto` par defaut des que `tools` est present.
- Preuve : liste des motifs lignes 50-60 de text_tools.rs ; test
  `recognizes_servers_refusing_tools` sans cas vLLM ; la boucle des refus (lignes 620-642)
  ne traite que les champs nommes avec leur orthographe exacte.
- Remede : ajouter aux motifs de `is_tools_unsupported_error` « tool choice requires » et
  « --enable-auto-tool-choice » (bascule et memoire du mode texte, deja en place), avec un cas
  de test ; dans l'aide d'erreur, citer `"toolMode": "text"` pour une erreur 400 qui parle
  d'outils.
- Etat : a verifier

