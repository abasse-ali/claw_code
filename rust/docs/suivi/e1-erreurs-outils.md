# E1. Erreurs rendues au modèle par les outils

Question : pour chaque erreur qu'un outil de Claw peut rendre à un modèle, le message dit-il ce
qui s'est passé, pourquoi, et quoi faire ensuite ? Claw pourrait-il réparer lui-même ?

Chaque constat est ajouté ici dès qu'il est confirmé dans le code, sous cette forme :

```
## E1-nn Titre court
- Gravité : haute | moyenne | basse
- Où : chemin:ligne
- Aujourd'hui : le message exact, et quand il arrive
- Défaut : ce qu'un petit modèle ne peut pas en tirer
- Remède : le nouveau texte, ou la reprise automatique à écrire
- État : à vérifier
```

« État » passe à « confirmé », « écarté » ou « corrigé (commit) » quand la session principale
le traite.

Étude faite le 11 octobre 2026 par lecture du code seulement (rien n'a été compilé ni lancé).
Les numéros de ligne sont ceux de la copie de travail de ce jour, après le commit bf47311 ;
`openai_compat.rs` y portait des modifications non committées. Les textes de serde, `regex`,
`glob`, `url` et `reqwest` cités ont été relus dans les sources de ces paquets (registre
Cargo local). 26 constats, suivis de ce qui a été examiné sans constat et d'un ordre de
traitement conseillé.

## E1-01 Argument du mauvais type : le message de serde part brut, sans nommer l'argument
- Gravité : haute
- Où : rust/crates/tools/src/lib.rs:909 (`from_value`), appelé par chaque branche de `execute_tool` (lib.rs:868 à 906)
- Aujourd'hui : `serde_json::from_value(input.clone()).map_err(|error| error.to_string())`. Le
  contrôle fait avant (`input_check::problem`) ne regarde que la présence des arguments
  obligatoires de premier niveau. Tout le reste arrive au modèle sous la forme de serde, par
  exemple pour `read_file` avec `{"path":"a.py","offset":"50"}` : `invalid type: string "50",
  expected usize` ; avec `"offset": -1` : `invalid value: integer `-1`, expected usize` ; pour
  `edit_file` avec `"replace_all":"true"` : `invalid type: string "true", expected a boolean`.
  Seul `WebFetch.offset` a une lecture tolérante (`lenient_position`, lib.rs:1285).
- Défaut : le message ne nomme ni l'outil ni l'argument fautif, parle de types Rust (`usize`,
  `u64`) et ne dit pas que rien n'a été exécuté. Un petit modèle qui écrit ses nombres entre
  guillemets (très fréquent en mode texte et avec Ollama) renvoie le même appel.
- Remède : réparer avant de refuser. Dans `from_value` (ou juste avant, à partir du schéma de
  l'outil), convertir sans perte ce qui est sans ambiguïté : chaîne faite de chiffres vers
  entier, `"true"`/`"false"` vers booléen, nombre à virgule entier (`50.0`) vers entier, entier
  négatif ramené à 0 pour `offset`, chaîne seule vers tableau à un élément quand le schéma
  attend un tableau de chaînes. Aucune requête de plus. Si la conversion est impossible, rendre
  un message qui nomme l'argument, par exemple avec `serde_path_to_error` ou en comparant au
  schéma :
  `[Claw] read_file was not executed: argument "offset" must be a whole number (for example 50), but a text value "abc" was sent. Send the call again with "offset" as a number, or leave it out.`
- État : corrigé (lot 4) : normalize_arguments convertit d'après le schéma ce qui est sans ambiguïté (nombre ou booléen en texte, nombre à virgule entier, liste ou objet reçus comme texte JSON, texte seul pour une liste de textes, valeur sous le minimum) et le dit avec le résultat ; une erreur de type restante dit que rien n'a tourné et quoi vérifier.

## E1-02 Outil qui n'existe pas : traité comme une question d'autorisation, puis « unsupported tool »
- Gravité : haute
- Où : rust/crates/runtime/src/permissions.rs:226 (`required_mode_for`), rust/crates/tools/src/lib.rs:317 et :905, rust/crates/claw-cli/src/main.rs:7071 (`invalid_input`), rust/crates/runtime/src/conversation.rs:1890 à 1947
- Aujourd'hui : un nom que `resolve_tool_name` (lib.rs:150) ne reconnaît pas (il ne connaît que
  `read`, `write`, `edit`, `multiedit`, `glob`, `grep` et les variantes de casse) suit le chemin
  normal. `invalid_input` rend `None` (définition introuvable), puis `authorize` lui attribue
  `.unwrap_or(PermissionMode::DangerFullAccess)`. Selon le mode, le modèle reçoit donc :
  en lecture seule `l'outil 'list_dir' nécessite la permission danger-full-access ; le mode
  actuel est read-only (lecture seule : explorez et proposez un plan sans modifier)` ; sans
  personne pour répondre `l'outil 'list_dir' nécessite une approbation pour passer de
  workspace-write à danger-full-access` ; en mode `prompt`, l'utilisateur se voit demander une
  autorisation « danger-full-access » pour un outil qui n'existe pas, et s'il accepte le modèle
  reçoit `unsupported tool: list_dir`.
- Défaut : aucun de ces textes ne dit que l'outil n'existe pas, ni lesquels existent. Les deux
  premiers font croire à un problème d'autorisation : le modèle abandonne ou demande à
  l'utilisateur de changer de mode. `unsupported tool: X` ne propose rien. L'utilisateur est
  dérangé pour un appel qui ne peut pas aboutir, et le refus compte comme un refus de
  l'utilisateur (`aids.refused`), ce qui coupe la persévérance.
- Remède : vérifier l'existence de l'outil avant l'autorisation (dans `invalid_input`, qui a
  déjà le registre), et rendre sans rien demander :
  `[Claw] There is no tool named "list_dir", so nothing was executed. Closest tools: glob_search (list files matching a pattern), bash (run a shell command such as ls). Tools you can call: bash, read_file, write_file, edit_file, glob_search, grep_search, ...`
  Les plus proches se calculent par distance sur les noms du registre plus une petite table de
  synonymes d'autres agents (`run_terminal_cmd`, `execute_command`, `shell` vers `bash` ;
  `list_dir`, `ls`, `list_files` vers `glob_search` ; `str_replace_editor`, `replace_in_file`
  vers `edit_file` ; `create_file`, `write_to_file` vers `write_file` ; `search_files`,
  `codebase_search`, `rg` vers `grep_search` ; `cat`, `view`, `open_file` vers `read_file`).
  Quand les arguments se traduisent un pour un (voir E1-04), Claw peut exécuter directement
  l'outil équivalent et le dire en tête du résultat. Aucune requête de plus.
- État : corrigé (lot 4) : un outil absent du registre reçoit, sans demande d'autorisation, unknown_tool_message (synonymes d'autres agents, noms voisins, liste des outils).

## E1-03 Sous-agents : les noms « Read », « Bash », « Grep » ne sont pas reconnus, et le refus ne dit pas quels outils sont permis
- Gravité : moyenne
- Où : rust/crates/tools/src/lib.rs:3220 (flux du sous-agent) et :3317 à 3320 (`SubagentToolExecutor::execute`)
- Aujourd'hui : l'agent principal fait passer chaque nom par `resolve_tool_name`
  (claw-cli/src/main.rs:6062 et :6142). Le sous-agent ne le fait pas :
  `events.push(AssistantEvent::ToolUse { id, name, input });`. Un appel `Read`, `Bash`, `Grep`
  ou `todowrite` d'un sous-agent (exploration et vérificateur d'ultracode compris) reçoit
  `tool `Read` is not enabled for this sub-agent`.
- Défaut : le message ne dit pas quels outils sont permis, ni que `read_file` est le nom
  attendu. Le modèle conclut qu'il ne peut pas lire de fichier et rend un rapport sans preuve.
  Le même modèle, comme agent principal, aurait vu son appel réussir. Même message pour un
  outil réellement absent du rôle : Explore et Plan n'ont pas `bash` (lib.rs:2886 à 2896), et
  un `bash` avec `ls` ou `find` y est le premier geste d'un petit modèle.
- Remède : résoudre le nom dans le sous-agent comme dans l'agent principal (une fonction libre
  partagée, puisque `resolve_tool_name` est une méthode du registre), puis, si l'outil reste
  hors de la liste, la donner :
  `[Claw] "write_file" is not available to this sub-agent, so nothing was executed. Tools you can call here: bash, read_file, glob_search, grep_search, FindSymbol, WebFetch. If the task needs a tool that is not listed, say so in your final report instead of retrying.`
  Aucune requête de plus.
- État : corrigé (lot 4) : les sous-agents résolvent les noms comme l'agent principal (resolve_name_among), et un outil hors de leur liste reçoit la liste des outils permis.

## E1-04 Nom d'argument voisin : « appelé sans ses arguments », ou argument ignoré en silence
- Gravité : haute
- Où : rust/crates/tools/src/input_check.rs:170 à 178 (`problem`), rust/crates/tools/src/lib.rs:1243 à 1273 (structures d'entrée, sans `deny_unknown_fields` ni `alias`), rust/crates/runtime/src/bash.rs:16 (`BashCommandInput`), rust/crates/runtime/src/file_ops.rs:108 (`GrepSearchInput`)
- Aujourd'hui : deux cas.
  1. Argument obligatoire sous un autre nom. `Read` devient bien `read_file`, mais un modèle qui
     écrit `Read` écrit aussi `{"file_path":"src/a.py"}`. Il reçoit : `` `read_file` was called
     without its arguments, so nothing was executed. It needs: path. Send the call again with
     them; if your reply was cut off before the arguments, send a smaller call. `` Pour
     `edit_file` avec `file_path`, `old_str`, `new_str` : même texte, alors que trois arguments
     ont été envoyés.
  2. Argument facultatif inconnu. Les schémas disent `"additionalProperties": false`, mais les
     structures l'acceptent et l'oublient. `read_file` avec `start_line`/`end_line` rend le
     début du fichier ; `grep_search` avec `include`, `case_insensitive` ou `ignore_case`
     cherche partout en respectant la casse ; `bash` avec `cwd` ou `workdir` lance la commande
     dans le dossier de travail ; `edit_file` avec `replaceAll` rend l'erreur d'ambiguïté. Rien
     ne le dit au modèle.
- Défaut : dans le cas 1 le message affirme une chose fausse (rien n'a été envoyé) et pousse à
  « envoyer un appel plus petit ». Dans le cas 2 le résultat est celui d'un autre appel que
  celui que le modèle croit avoir fait : il raisonne sur un résultat faux sans le savoir.
- Remède : une table de synonymes par outil, appliquée avant `problem` quand le nom officiel
  est absent : `file_path`, `filepath`, `filename`, `file` vers `path` ; `old_str`, `old_text`,
  `search` vers `old_string` ; `new_str`, `new_text`, `replace` vers `new_string` ;
  `replaceAll` vers `replace_all` ; `contents`, `text`, `file_text` vers `content` ; `cmd`,
  `script` vers `command` ; `regex`, `query` vers `pattern` ; `include` vers `glob` ;
  `case_insensitive`, `ignore_case` vers `-i` ; `timeout_ms` vers `timeout` ; `start_line` et
  `end_line` vers `offset` et `limit` (offset = start_line - 1). Pour `cwd`/`workdir` de
  `bash`, préfixer la commande par `cd "<dossier>" && `. Ce qui reste inconnu est dit en tête
  du résultat, sans bloquer :
  `[Claw] Ignored unknown argument(s) "foo". read_file accepts: path, offset, limit.`
  Et quand un obligatoire manque alors que des inconnus ont été reçus, le message les cite :
  `[Claw] edit_file was not executed: required argument "path" is missing. You sent "target", which edit_file does not know. It accepts: path, old_string, new_string, replace_all.`
  Aucune requête de plus.
- État : corrigé (lot 4) : synonymes d'arguments (file_path, old_str, cmd, include...), start_line et end_line, cwd de bash ; les arguments inconnus restants sont nommés avec le résultat, et un obligatoire manquant cite ceux qui ont été envoyés sous un autre nom.

## E1-05 Lire, modifier ou écrire un dossier : l'erreur brute du système (« Accès refusé. (os error 5) »)
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:330 à 355 (`read_text`), :357 (`read_file`), :404 à 434 (`write_file`), :452 (`edit_file`), rust/crates/tools/src/lib.rs:1238 (`io_to_string`)
- Aujourd'hui : aucun contrôle « est-ce un dossier » (le fichier ne contient `is_dir` que pour
  les recherches). `read_file` avec `path: "src"`, `"."` ou `""` (le chemin vide donne le
  dossier de travail) passe `normalize_path`, puis `fs::read(path)?` échoue et
  `io_to_string` rend `error.to_string()` : sous Windows le texte du système dans sa langue,
  du genre `Accès refusé. (os error 5)` ; sous Linux `Is a directory (os error 21)`.
  `write_file` vers un dossier existant échoue de même sur `fs::write(&absolute_path, bytes)?`.
- Défaut : le message ne cite ni le chemin ni la cause. Sous Windows il fait croire à un
  problème de droits : le modèle demande à l'utilisateur de lancer Claw en administrateur, ou
  abandonne la lecture. Lire un dossier pour « voir ce qu'il contient » est un des premiers
  gestes d'un petit modèle.
- Remède : reprise par Claw. Dans `read_file`, si le chemin est un dossier, rendre un résultat
  (pas une erreur) qui liste ses entrées, dossiers marqués d'un `/`, 200 au plus, avec :
  `[Claw] "src" is a directory, not a file, so here is what it contains. Call read_file on one of these files, or glob_search to look deeper.`
  Dans `edit_file` et `write_file`, une erreur qui nomme le chemin :
  `[Claw] "src/utils" is a directory: write_file needs the path of a file inside it, for example src/utils/helpers.py.`
  Aucune requête de plus.
- État : corrigé (lot 3) : read_file liste un dossier (200 entrées au plus, dossiers marqués d'un /) au lieu de l'erreur du système ; write_file et edit_file sur un dossier disent qu'il faut un fichier dedans.

## E1-06 grep_search : un motif qui n'est pas une expression régulière valide rend l'erreur brute du moteur
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:874 à 878
- Aujourd'hui : `.build().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?`.
  Chercher un appel (`print(`), un indice (`items[`), une accolade (`${name}`) ou `**kwargs`
  rend le texte du moteur `regex`, sur plusieurs lignes, de la forme
  `regex parse error:` / le motif / un `^` sous la faute / `error: unclosed group`. Un motif
  avec `(?!...)` ou `(?<=...)` rend `error: look-around, including look-ahead and look-behind,
  is not supported`.
- Défaut : le texte dit ce qui cloche dans la syntaxe, pas quoi faire. La description de
  l'outil demande d'échapper `(`, `{`, `.`, mais un petit modèle cherche du code tel qu'il
  l'écrit. Il réessaie avec une autre faute, ou conclut que la recherche ne marche pas et
  passe à `bash grep`. Chaque essai coûte une requête.
- Remède : reprise par Claw. Si le motif ne compile pas, le chercher comme texte littéral
  (`regex::escape`) et le dire dans `note`, le résultat restant un succès :
  `[Claw] "print(" is not a valid regular expression (unclosed group), so it was searched as plain text. To use a regex, escape special characters: print\(`
  Pour un motif avec `(?=`, `(?!`, `(?<=`, `(?<!` ou `\1` (non pris en charge par le moteur),
  la recherche littérale n'a pas de sens : rendre une erreur qui dit quoi faire :
  `[Claw] grep_search does not support look-ahead, look-behind or backreferences. Search for the simpler part of the pattern here, or run grep -P in bash.`
  Aucune requête de plus.
- État : corrigé (lot 3) : un motif invalide est cherché comme texte, et la note le dit ; regards avant, arrière et références arrière reçoivent une erreur qui dit quoi faire.

## E1-07 grep_search : trois filtres qui rendent « rien trouvé » sans rien dire
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:1216 à 1236 (`matches_optional_filters`), :887 à 890 et :911 à 941 (`output_mode`), rust/crates/tools/src/lib.rs:431 et :438 (schéma)
- Aujourd'hui :
  1. `type` est comparé à l'extension : `if extension != Some(file_type) { return false; }`.
     Le schéma dit seulement `"type": { "type": "string" }`. Avec `type: "python"`, `"rust"`,
     `"typescript"`, `"javascript"` (les noms de ripgrep, que les modèles connaissent) ou
     `".py"`, aucun fichier ne passe.
  2. `glob` est comparé au chemin absolu du fichier : `glob_filter.matches(&path_string)` puis
     `matches_path(path)`. `**/*.py` et `*.py` passent, mais `src/**/*.ts` ou `tests/*.py` ne
     peuvent jamais correspondre à `D:\projet\src\a.ts`. Les accolades (`*.{ts,tsx}`) ne sont
     pas développées par le paquet `glob` : elles sont prises à la lettre.
  3. `output_mode` est un texte libre : tout ce qui n'est ni `content` ni `count` se comporte
     comme `files_with_matches`, et la valeur reçue est renvoyée telle quelle dans `mode`.
     `"lines"`, `"contents"`, `"matches"` rendent donc des noms de fichiers sans le dire.
  Dans les cas 1 et 2, le résultat est `numFiles: 0`, `filenames: []`, sans `note` (la note
  ne parle que des dossiers sautés et des gros fichiers).
- Défaut : un résultat vide passe pour une absence. Le modèle conclut que le symbole n'existe
  pas dans le projet et écrit du code en double, ou annonce à l'utilisateur que « rien
  n'utilise cette fonction ». C'est l'erreur silencieuse la plus coûteuse de l'outil.
- Remède : réparer puis dire.
  1. `type` : retirer le point initial et traduire les noms courants en extensions (`python`
     vers `py`, `pyi` ; `rust` vers `rs` ; `typescript`, `ts` vers `ts`, `tsx` ; `javascript`,
     `js` vers `js`, `jsx`, `mjs`, `cjs` ; `markdown` vers `md` ; `yaml` vers `yml`, `yaml` ;
     `cpp`, `c++` vers `cpp`, `cc`, `hpp`, `h` ; `shell`, `sh` vers `sh`, `bash`).
  2. `glob` : comparer au chemin relatif à la base de recherche, avec des `/`, et développer
     les accolades avant de compiler (un motif par alternative).
  3. `output_mode` : accepter les synonymes (`lines`, `contents`, `matches` vers `content` ;
     `files`, `files_only` vers `files_with_matches`) et déclarer l'`enum` dans le schéma.
  4. Dans tous les cas, quand un filtre a écarté tous les fichiers parcourus, le dire :
     `[Claw] No file was searched: 214 files were found under src but none passed the filter type "golang". Use a file extension (go), or drop the filter.`
  Aucune requête de plus.
- État : corrigé (lot 3) : type lu comme langage ou extension, glob comparé au chemin sous la base et au nom, accolades développées, synonymes de output_mode lus et dits (et enum dans le schéma), note quand les filtres ont tout écarté.

## E1-08 Mode texte : un bloc `<tool_call>` au JSON invalide ou coupé est jeté sans rien dire
- Gravité : haute
- Où : rust/crates/api/src/providers/text_tools.rs:130 à 155 (`extract_tool_calls`), :195 (`parse_call`), :226 à 233 (`lenient_json`), :463 à 474 (`TextToolFilter::push`), rust/crates/api/src/providers/openai_compat.rs:1114 à 1130 (`finish_text_tools`)
- Aujourd'hui : en mode texte (modèles sans appel d'outils natif, Ollama surtout), tout ce qui
  suit la première balise `<tool_call>` est masqué (`self.hidden = true`), puis chaque bloc est
  lu par `parse_call`. `lenient_json` n'accepte que du JSON valide entre la première `{` et la
  dernière `}`. Un bloc avec un retour à la ligne brut dans une chaîne (le cas courant d'un
  `write_file` écrit à la main), des guillemets simples, une virgule finale, `True`, ou coupé
  par la limite de sortie (balise non refermée) donne `None`, et
  `if let Some(call) = parse_call(body) { calls.push(call); }` l'oublie. La réparation de
  `json_repair.rs` ne sert pas ici : elle ne s'applique qu'aux appels déjà reconnus
  (conversation.rs:2051).
- Défaut : aucun appel, aucun texte, aucune erreur. Le modèle a écrit « Je crée le fichier »
  puis son bloc : le tour se termine sur cette phrase, l'utilisateur croit le fichier créé. Sans
  texte avant le bloc, la réponse compte comme vide et repart une fois avec
  `EMPTY_RESPONSE_NUDGE` (conversation.rs:474), qui parle d'un appel « written only in your
  reasoning » : ce n'est pas la cause, et le modèle renvoie le même bloc.
- Remède :
  1. Reprise par Claw : faire passer le corps du bloc par la même réparation que les appels
     natifs (déplacer `repair_tool_input` dans une caisse commune à `api` et `runtime`).
  2. Si le bloc reste illisible, ne pas le jeter : émettre un appel au nom lu par un motif
     simple (`"name"\s*:\s*"([A-Za-z_]+)"`) avec une marque que `input_check::problem` traduit,
     comme pour les appels coupés :
     `[Claw] Your <tool_call> block for write_file was not valid JSON (control character inside a string, line 3), so nothing was executed. Inside a JSON string write a line break as \n and a double quote as \". Send the block again.`
     Et, bloc sans balise de fin avec arrêt à la limite de sortie, le texte déjà prévu :
     `Your reply reached the output limit before the arguments of ...`.
  Aucune requête de plus : aujourd'hui le tour s'arrête à tort ou repart à l'aveugle.
- État : à vérifier

## E1-09 Réparation du JSON : quatre fautes courantes non réparées, et une réparation qui exécute un appel tronqué
- Gravité : moyenne
- Où : rust/crates/runtime/src/json_repair.rs:43 à 125 (`scan_and_fix`), rust/crates/tools/src/input_check.rs:103 à 105 et :160 à 164, rust/crates/runtime/src/file_ops.rs:1315 (`candidate_path`)
- Aujourd'hui : `scan_and_fix` répare le bloc de code, le texte autour, les virgules finales,
  les retours à la ligne bruts, `True`/`False`/`None` et la troncature. Il ne répare pas :
  1. les guillemets simples à la Python (`{'path': 'a.py'}`) ;
  2. les clés sans guillemets (`{path: "a.py"}`) ;
  3. une barre oblique inverse devant un caractère qui n'est pas un échappement JSON : motif
     `"\d+"`, `"\."`, chemin Windows `"D:\Mes_Projets\app.py"` (la branche `'\\'` recopie la
     barre et le caractère suivant tels quels) ;
  4. un chemin Windows dont les barres forment des échappements valides :
     `"C:\temp\new\file.txt"` est du JSON valide et donne un chemin avec une tabulation, un
     saut de ligne et un saut de page. `candidate_path` le prend tel quel.
  Dans les cas 1 à 3 le modèle reçoit `invalid tool input JSON: <message de serde> - nothing
  was executed, send the call again with a valid JSON object`, par exemple `invalid escape at
  line 1 column 22`. Dans le cas 4, `File not found:` suivi d'un chemin troué, ou sous Windows
  l'erreur brute du système pour un nom incorrect, sans le chemin (les caractères de contrôle
  y sont interdits dans un nom ; `normalize_path` ne traite que `NotFound`).
  À l'inverse, une sortie tronquée est refermée (`if in_string { ... out.push('"'); }`) puis
  exécutée. `check_cut_calls` l'arrête quand l'arrêt est `length` ou autre, mais rend la main
  tout de suite si `stop_reason == Some("tool_use")` : dans ce cas un `write_file` ou un
  `bash` aux arguments tronqués serait exécuté tel quel. Je n'ai pas vérifié qu'un fournisseur
  envoie cette combinaison.
- Défaut : « line 1 column 22 » ne dit pas à un petit modèle quel caractère changer. Et aucune
  réparation n'est signalée : le modèle ne sait pas que son appel a été corrigé, donc il
  refait la même faute à l'appel suivant.
- Remède :
  1. Réparer en plus : guillemets simples quand le texte ne contient aucun guillemet double
     de structure, clés nues, et barre oblique inverse doublée quand le caractère suivant
     n'est pas l'un de `" \ / b f n r t u`.
  2. Dans un argument de chemin (`path`, `notebook_path`), remplacer un caractère de contrôle
     par la séquence d'origine (tabulation vers `\t`, saut de ligne vers `\n`...) avant de
     chercher le fichier.
  3. Quand la réparation a dû refermer une chaîne ou une accolade, ne pas exécuter un outil
     qui écrit ou lance une commande : rendre le message d'appel coupé.
  4. Message quand rien n'est réparable, avec l'extrait fautif :
     `[Claw] The arguments of grep_search were not valid JSON, so nothing was executed. Problem near: "pattern": "\d+" (a backslash must be written twice inside a JSON string: "\\d+"). Send the call again.`
  Aucune requête de plus.
- État : à vérifier

## E1-10 glob_search : motifs courants qui rendent zéro fichier sans explication, et erreur de motif brute
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:781 à 864 (`glob_search`), en particulier :787 à 795 et :854 à 863
- Aujourd'hui :
  1. Le motif est collé au dossier de base puis donné au paquet `glob` :
     `display_path(&base_dir.join(pattern))`. Rien n'échappe le dossier de base : avec
     `path: "app/[slug]"` (dossier de route Next.js) les crochets sont lus comme une classe de
     caractères et rien n'est trouvé. De même un projet rangé dans `D:\Projets [2026]\`.
  2. Les accolades ne sont pas développées : `**/*.{ts,tsx}` cherche un fichier dont
     l'extension est littéralement `{ts,tsx}`.
  3. `*.py` ne descend pas dans les sous-dossiers ; `src` ou `src/` (un dossier) ne rend rien
     car `if !entry.is_file() { continue; }`.
  4. Motif invalide (`**.py`, `src/**.ts`) : `.map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))`
     rend le texte du paquet, de la forme `Pattern syntax error near position 31: recursive
     wildcards must form a single path component`. La position compte dans le chemin absolu
     que le modèle n'a jamais écrit.
  5. Résultat vide : `note` n'est remplie que si des dossiers ont été sautés
     (`search_note(&skipped, 0)`). Sinon `{"numFiles": 0, "filenames": [], "truncated": false}`.
  6. Plus de 100 fichiers : `truncated: true`, sans le total ni conseil.
- Défaut : zéro fichier passe pour « ce type de fichier n'existe pas ». Dans les cas 1 à 3 le
  modèle ne peut pas deviner que c'est son motif qui est en cause. Dans le cas 4 il ne sait pas
  quelle forme est attendue.
- Remède : reprises par Claw, chacune dite dans `note`.
  1. Échapper le dossier de base (`glob::Pattern::escape`) avant d'y joindre le motif. Si le
     motif lui-même contient `[...]` et qu'un dossier de ce nom exact existe, l'échapper aussi.
  2. Développer les accolades en plusieurs motifs et réunir les résultats.
  3. Zéro fichier et pas de `**` dans le motif : relancer avec `**/` devant, et dire :
     `[Claw] Nothing matches "*.py" in the top folder, so the search was repeated in all subfolders ("**/*.py"): 14 files.`
     Motif qui nomme un dossier existant : lister `dossier/**/*` et le dire.
  4. Réécrire les formes fautives connues (`**.ext` vers `**/*.ext`) ; sinon message :
     `[Claw] "src/**.ts" is not a valid glob: ** must stand alone between slashes. Did you mean "src/**/*.ts"?`
  5. Zéro fichier sans autre cause : dire ce qui a été parcouru :
     `[Claw] No file matches "**/*.vue" under D:/projet (1 240 files looked at). Check the extension, or list what exists with "**/*".`
  6. `truncated` : ajouter `Showing the 100 most recent of 2 316 matching files: narrow the pattern or give a path.`
  Aucune requête de plus.
- État : corrigé (lot 3) : glob_search parcourt la base comme grep_search et compare le chemin relatif ; accolades, ** collé réparé, motif sans ** puis dossier nommé tels quels cherchés plus largement, note quand rien ne correspond (fichiers parcourus) ou quand la liste est coupée.

## E1-11 TodoWrite : erreurs brutes de serde sur chaque tâche, et un fichier de suivi illisible bloque l'outil pour toujours
- Gravité : haute
- Où : rust/crates/tools/src/lib.rs:1304 à 1322 (`TodoWriteInput`, `TodoItem`, `TodoStatus`), :2111 à 2121 (`execute_todo_write`), :2171 à 2183 (`validate_todos`)
- Aujourd'hui :
  1. `input_check::problem` ne contrôle que `todos`. Le contenu de chaque tâche est lu par
     serde : sans `activeForm`, `missing field `activeForm`` ; avec `"status": "done"`,
     `unknown variant `done`, expected one of `pending`, `in_progress`, `completed`` ; avec une
     liste de textes (`["Fix the bug", "Add a test"]`), `invalid type: string "Fix the bug",
     expected struct TodoItem` ; avec la liste envoyée comme texte JSON (`"todos": "[{...}]"`),
     `invalid type: string "[{...", expected a sequence`.
  2. L'ancienne liste est relue avant d'écrire :
     `serde_json::from_str::<Vec<TodoItem>>(&std::fs::read_to_string(&store_path)...).map_err(|error| error.to_string())?`.
     Si `.claw/todos.json` est vide (écriture interrompue : `std::fs::write` n'est pas
     atomique), abîmé, ou d'une ancienne forme, chaque appel échoue avec par exemple
     `EOF while parsing a value at line 1 column 0`.
  3. `todos: []` rend `todos must not be empty`.
- Défaut : (1) aucun message ne dit quelle tâche est fautive ni ce que vaut `activeForm`.
  `TodoWrite` est l'outil que la consigne impose pour tout travail de plus de deux étapes : un
  modèle qui échoue deux fois dessus cesse de tenir sa liste. (2) Le modèle ne peut rien y
  faire : il ne connaît pas ce fichier, et l'erreur ne le nomme pas. L'outil reste cassé pour
  ce projet jusqu'à ce que l'utilisateur supprime le fichier. (3) Le modèle qui veut dire
  « tout est fini » ne sait pas qu'il faut renvoyer les tâches en `completed`.
- Remède : reprises par Claw.
  1. Lire chaque tâche avec tolérance : `activeForm` absent ou vide prend la valeur de
     `content` ; `status` absent vaut `pending` ; synonymes (`done`, `complete`, `finished`
     vers `completed` ; `in-progress`, `inprogress`, `doing`, `active`, `started` vers
     `in_progress` ; `todo`, `open`, `not_started` vers `pending`) ; clés voisines (`task`,
     `title`, `text`, `description` vers `content` ; `state` vers `status`) ; un texte seul
     devient une tâche `pending` ; une liste reçue comme texte JSON est décodée. Ce dernier
     point vaut pour tout argument de type tableau ou objet (voir E1-01).
  2. Fichier illisible : repartir d'une liste vide (`old_todos = []`) au lieu d'échouer, et
     écrire par fichier temporaire puis renommage.
  3. `[Claw] The list was not changed: an empty list is not accepted. To close the list, send every task again with "status": "completed".`
  Si une tâche reste illisible :
  `[Claw] TodoWrite was not executed: task 3 has no "content". Each task is {"content": "Fix the login test", "activeForm": "Fixing the login test", "status": "pending" | "in_progress" | "completed"}.`
  Aucune requête de plus.
- État : corrigé (lot 4) : TodoWrite lit la liste avec tolérance (texte JSON, textes seuls, clés et statuts voisins, activeForm absent), repart d'une liste vide si le fichier est illisible, écrit par fichier temporaire, et dit quoi faire pour une liste vide ou une tâche illisible.

## E1-12 Sous-agent qui échoue ou atteint ses 32 étapes : tout son travail est perdu, et le parent reçoit une phrase
- Gravité : haute
- Où : rust/crates/tools/src/lib.rs:2271 (`DEFAULT_AGENT_MAX_ITERATIONS: usize = 32`), :2338 à 2357 (`execute_agent`), rust/crates/runtime/src/conversation.rs:773 à 777
- Aujourd'hui : à la 33e étape, `run_turn` rend
  `Err(RuntimeError::new("la boucle de conversation a dépassé le nombre maximal d'itérations"))`
  et l'outil `Agent` rend au modèle parent
  `le sous-agent `Explore` a échoué : la boucle de conversation a dépassé le nombre maximal d'itérations`.
  Il en va de même pour une erreur du fournisseur à n'importe quelle étape
  (`le sous-agent `X` a échoué : {error}`).
- Défaut : jusqu'à 32 requêtes du quota gratuit sont dépensées et rien n'en revient : ni les
  fichiers lus, ni les constats, ni le dernier texte du sous-agent. Le message ne dit pas quoi
  faire. Un petit modèle relance le même sous-agent avec la même tâche (32 requêtes de plus),
  ou abandonne l'exploration. Les étapes d'exploration et de vérification d'ultracode passent
  par ce chemin.
- Remède : à la limite, ne pas rendre d'erreur. Une dernière requête au sous-agent, sans
  outils (une requête de plus, pour en sauver 32) :
  `[Claw] You have used your 32 steps. Do not call any tool. Write your final report now: what you established (with files and lines), and what remains unchecked.`
  Si cette requête échoue aussi, ou pour une panne du fournisseur en cours de route, rendre
  au parent ce qui existe, sans nouvelle requête : le dernier texte du sous-agent, la liste
  des fichiers lus et des commandes lancées, et :
  `[Claw] The sub-agent Explore stopped before writing its report (provider error 429 at step 14 of 32). What it had done so far is above. Do not start the same task again as it is: continue from these notes yourself, or delegate a smaller part.`
- État : à vérifier

## E1-13 Agent : les rapports sont écrits dans le grand-parent du projet, et l'échec de ce dossier est une erreur brute
- Gravité : moyenne
- Où : rust/crates/tools/src/lib.rs:3674 à 3683 (`agent_store_dir`), :2383 à 2384 et :2446 (`prepare_agent_job`)
- Aujourd'hui : sans `CLAW_AGENT_STORE` (que rien ne définit hors des tests), le dossier est
  `cwd.ancestors().nth(2)` puis `.claw-agents` : le grand-parent du dossier de travail. Pour
  un projet `D:\Mes_Projets\app`, c'est `D:\.claw-agents`. Pour `C:\Users\HP\app` ou
  `/home/moi/app`, c'est `C:\Users\.claw-agents` ou `/home/.claw-agents`, où un compte
  ordinaire ne peut pas écrire :
  `std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?` rend alors
  `Accès refusé. (os error 5)` ou `Permission denied (os error 13)`.
- Défaut : le modèle reçoit une erreur sans chemin pour un appel `Agent` correct. Il ne peut
  rien corriger, et tous les sous-agents échouent de la même façon pour ce projet. Quand cela
  marche, le `outputFile` rendu est hors du projet, à un endroit que le README dit ne pas
  utiliser (« rien à la racine »).
- Remède : écrire dans `<dossier de travail>/.claw/agents/`. Si la création échoue, se rabattre
  sur le dossier temporaire du système, sans erreur pour le modèle. Aucune requête de plus.
- État : à vérifier

## E1-14 NotebookEdit : sans `cell_id` la dernière cellule est remplacée ou supprimée, et les erreurs ne disent pas quelles cellules existent
- Gravité : moyenne
- Où : rust/crates/tools/src/lib.rs:3748 à 3782 (`execute_notebook_edit`), :4361 à 4382 (`resolve_cell_index`)
- Aujourd'hui :
  1. `resolve_cell_index` sans `cell_id`, en mode `replace` ou `delete` :
     `Ok(cells.len().saturating_sub(1))`. Un appel `{notebook_path, new_source}` écrase la
     dernière cellule ; `{notebook_path, edit_mode: "delete"}` la supprime. Le résultat est un
     succès.
  2. `Cell id not found: {cell_id}` : ni la liste des identifiants, ni le nombre de cellules.
     Un notebook d'avant nbformat 4.5 n'a aucun identifiant : aucune cellule n'y est jamais
     trouvée. Un index (`"3"`, `"cell-3"`) n'est pas compris.
  3. Le chemin est pris tel quel (`std::path::PathBuf::from(&input.notebook_path)`), sans
     `normalize_path` : pas d'aide aux chemins. Fichier absent :
     `std::fs::read_to_string(&path).map_err(|error| error.to_string())?` rend le texte brut du
     système, du genre `Le fichier spécifié est introuvable. (os error 2)`, sans le chemin.
     Notebook vide ou abîmé : texte brut de serde (`expected value at line 1 column 1`).
  4. `Notebook cells array not found`, `Notebook has no cells to edit`, `File must be a Jupyter
     notebook (.ipynb file).` : aucune suite proposée.
- Défaut : (1) est une perte de données silencieuse. (2) à (4) laissent le modèle sans moyen
  de viser la bonne cellule ; il réécrit alors tout le notebook avec `write_file` et casse
  souvent le JSON.
- Remède :
  1. Sans `cell_id` en `replace` ou `delete` : refuser et lister les cellules.
  2. Accepter un index (`3`, `"3"`, `"cell-3"`) quand aucun identifiant ne correspond. Dans
     toute erreur de cellule, joindre la liste :
     `[Claw] No cell has the id "intro". This notebook has 4 cells: 0 markdown "# Sales report", 1 code "import pandas as pd", 2 code "df = pd.read_csv(...)", 3 code "df.plot()". Give "cell_id" as one of these numbers.`
  3. Passer par `normalize_path` (chemin retrouvé, fichiers proches). Fichier absent :
     `[Claw] notes.ipynb does not exist. NotebookEdit changes an existing notebook; to create one, write its JSON with write_file.`
  Aucune requête de plus.
- État : à vérifier

## E1-15 WebFetch : adresse sans « http », serveur local arrêté, page 404 : trois issues sans explication
- Gravité : haute
- Où : rust/crates/tools/src/lib.rs:1706 à 1745 (`execute_web_fetch`), :1889 à 1902 (`normalize_fetch_url`), rust/crates/tools/src/web_page.rs:51 à 52 (`render`)
- Aujourd'hui :
  1. Adresse sans schéma : `reqwest::Url::parse(url).map_err(|error| error.to_string())?`
     rend `relative URL without a base` pour `docs.python.org/3/`. `localhost:8000` est lu
     comme une adresse de schéma « localhost » et échoue plus loin (`builder error for url
     (localhost:8000)`).
  2. Échec réseau : `.send().map_err(|error| error.to_string())?`. Avec reqwest 0.12.28,
     le texte est `error sending request for url (http://localhost:8000/)` : la cause
     (connexion refusée, nom introuvable, délai de 20 s, TLS) reste dans `source()` et n'est
     pas écrite.
  3. Passage forcé en https : toute adresse `http://` dont l'hôte n'est pas exactement
     `localhost`, `127.0.0.1` ou `::1` devient `https://`. `http://0.0.0.0:8000` (ce
     qu'affichent Flask, uvicorn et Vite avec `--host`), `http://192.168.1.20:3000` ou
     `http://monposte:8080` échouent donc, avec dans l'erreur une adresse `https` que le modèle
     n'a pas écrite.
  4. Réponse 404, 403, 429 ou 500 : résultat réussi. La page d'erreur est rendue comme le
     contenu, précédée seulement de `Fetched {url} (HTTP 404 Not Found)`.
- Défaut : (1) et (2) ne disent pas quoi corriger. (2) est le cas de la session du 11 octobre
  (point A4 du suivi) : le serveur était arrêté et le modèle a quand même donné l'adresse à
  l'utilisateur. (3) fait conclure que le serveur local ne marche pas alors qu'il répond.
  (4) fait prendre « Page not found » ou une page de blocage pour la documentation demandée.
- Remède :
  1. Reprise : ajouter `https://` à un nom avec un point, `http://` à `localhost`, à une
     adresse IP ou à un nom sans point.
  2. Parcourir la chaîne `source()` de l'erreur et dire la cause. Pour une adresse locale,
     joindre l'état des tâches de fond (`bash_tasks::list()`) :
     `[Claw] Nothing answers on localhost:8000 (connection refused): no server is listening on that port. Background commands: bash_2 "uvicorn app:app" exited with code 1 after 3 s. Read its output with BashOutput, fix it, start it again in the background, then fetch.`
     Nom introuvable : `[Claw] The host api.exemple.dev does not exist (DNS lookup failed). Do not guess addresses: find the right one with WebSearch.`
     Délai : `[Claw] No answer from ... within 20 s.`
  3. Ne pas passer en https une adresse IP, un nom sans point ou `host.docker.internal` ;
     lire `0.0.0.0` comme `127.0.0.1`. Quand l'adresse a été changée, le dire dans le
     résultat.
  4. Code 400 et plus : mettre en tête
     `[Claw] The server answered 404 Not Found: this address does not exist. The text below is its error page, not the content you wanted. Check the address, or look for the page with WebSearch.`
  Aucune requête de plus.
- État : à vérifier

## E1-16 WebSearch : une recherche bloquée par le moteur est rendue comme « aucun résultat »
- Gravité : moyenne
- Où : rust/crates/tools/src/lib.rs:1821 à 1832 (moteur par défaut dans `search_hits`), :1840 à 1855 (`execute_web_search`), :1776 à 1778, :1797, :1811
- Aujourd'hui :
  1. Sans clé d'API, la page HTML de DuckDuckGo est lue sans contrôle du code de réponse :
     `let html = response.text()...; let mut hits = extract_search_hits(&html);`. Une page de
     contrôle anti-robot ou de limite de débit ne contient aucun résultat, et le modèle reçoit
     `No web search results matched the query "..." (DuckDuckGo).` Si la page contient
     d'autres liens, `extract_search_hits_from_generic_links` les rend comme des résultats.
  2. `allowed_domains` et `blocked_domains` sont appliqués après coup (`hits.retain(...)`) :
     si le filtre retire tout, même phrase, sans dire que des résultats existaient.
  3. Erreurs des moteurs configurés : `SearXNG : {error} (activez le format json dans
     settings.yml)`, `Brave Search : {error}`, `Tavily : {error}`, où `{error}` est le texte
     de reqwest (`HTTP status client error (429 Too Many Requests) for url (...)`). Le
     premier s'adresse à l'utilisateur, en français.
- Défaut : « aucun résultat » fait conclure que le sujet n'existe pas sur le web : le modèle
  répond alors de mémoire, ce que la recherche devait éviter. Dans le cas 3 le modèle ne peut
  rien faire d'un conseil de configuration.
- Remède :
  1. Contrôler le code de réponse et reconnaître la page de blocage (code autre que 200, ou
     aucune balise de résultat dans une page non vide), puis une seule nouvelle tentative
     après 2 s. En cas d'échec :
     `[Claw] The search engine refused this request (rate limit or bot check), so this is not "no results". Do not answer from memory as if you had searched: fetch a known documentation address with WebFetch, or say that the web search is unavailable right now.`
     Et prévenir l'utilisateur une fois qu'une clé Brave, Tavily ou une instance SearXNG rend
     la recherche fiable.
  2. Filtre qui retire tout :
     `[Claw] 8 results were found but none is on the allowed domains (docs.rs). The first ones were: ... Widen "allowed_domains" or drop it.`
  3. Erreur d'un moteur configuré : se rabattre sur DuckDuckGo au lieu d'échouer, et prévenir
     l'utilisateur (pas le modèle) du réglage à corriger.
  Aucune requête de modèle de plus.
- État : à vérifier

## E1-17 Lecture seule : `grep`, `find` et `cd` sont refusés, par un message en français qui parle de « proposer un plan »
- Gravité : haute
- Où : rust/crates/runtime/src/permissions.rs:270 à 279 (refus en lecture seule), :303 à 309 (refus sans personne pour répondre), :500 à 529 (`READ_ONLY_COMMANDS`), rust/crates/tools/src/lib.rs:2850 (`READ_ONLY_ROLE_TOOLS`)
- Aujourd'hui : en mode `read-only` (mode plan de l'agent principal, et sous-agents architect,
  reviewer et auditor, qui gardent `bash` pour les commandes sans effet), une commande `bash`
  ne passe que si chaque
  morceau commence par un nom de `READ_ONLY_COMMANDS` : `pwd`, `ls`, `dir`, `cat`, `head`,
  `tail`, `wc`, `which`, `where`, `echo`, `date`, quelques `git` et `--version`. `grep`, `rg`,
  `find`, `tree`, `stat`, `sort`, `diff`, `git blame`, `git grep` et même `cd` n'y sont pas :
  `cd src && ls` est refusé. Le modèle reçoit
  `l'outil 'bash' nécessite la permission danger-full-access ; le mode actuel est read-only (lecture seule : explorez et proposez un plan sans modifier)`.
  Sans personne pour répondre (sous-agent en arrière-plan), il reçoit
  `l'outil 'bash' nécessite une approbation pour passer de workspace-write à danger-full-access`.
- Défaut : le message ne dit pas quelles commandes passent ni quel outil prendre à la place
  (`grep_search`, `glob_search`). Pour un sous-agent de relecture ou d'audit, « proposez un
  plan » n'a pas de sens. Le second message ne donne aucune suite : le modèle réessaie la même
  commande sous une autre forme, ou par `PowerShell`. Chaque refus coûte une requête. (Les
  sous-agents Explore et Plan n'ont pas `bash` du tout, lib.rs:2886 à 2896 : un appel `bash`
  y reçoit le message de E1-03.)
- Remède :
  1. Reprise : ajouter à la liste les commandes qui ne changent rien (`cd`, `grep`, `rg`,
     `find` sans `-delete` ni `-exec`, `tree`, `stat`, `file`, `sort`, `uniq`, `cut`, `diff`,
     `git blame`, `git grep`, `git branch` sans argument, `git tag` sans argument).
  2. Message de refus en lecture seule, qui nomme la commande et la suite :
     `[Claw] bash was not run: this session is read-only and "npm test" may change files. Allowed here: ls, cat, head, tail, wc, grep, find, git status, git diff, git log, git show. To search, use grep_search and glob_search. Do not retry it in another form or with another tool.`
     En mode plan (agent principal), ajouter :
     `When your plan is complete, submit it with ExitPlanMode: the user's approval lets you edit.`
     Pour un sous-agent : `Put in your report what you could not run.`
  3. Refus sans personne pour répondre, sur le modèle de celui de `stream_json.rs:134` :
     `[Claw] bash was not run: this command needs the user's approval and nobody can answer in this session. Do not retry it, even in another form: continue without it and say at the end what is waiting for approval.`
  Aucune requête de plus.
- État : à vérifier

## E1-18 edit_file : quatre échecs fréquents dont le message laisse le modèle refaire une requête
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:460 à 471 (`edit_file`), :612 à 616 (`ambiguous_message`), :731 à 755 (`not_found_message`)
- Aujourd'hui :
  1. Texte ambigu : `old_string appears {count} times in the file: add surrounding lines to
     make it unique, or set replace_all to true`. Les lignes des occurrences ne sont pas
     données.
  2. Texte introuvable : `old_string not found in file (even ignoring whitespace and
     indentation). Closest line 42: `...`. Re-read the file with read_file and copy the exact
     text.` Une seule ligne proche est montrée, pour la première ligne cherchée seulement, et
     rien si aucune ligne ne dépasse 0,5 de ressemblance. Le conseil coûte une requête.
  3. `old_string and new_string must differ` : rien sur la suite.
  4. `old_string must not be empty (use write_file to create or overwrite a file)`. Ce contrôle
     vient après `normalize_path(path)?` : pour créer un fichier par `edit_file` avec
     `old_string: ""`, le modèle reçoit d'abord `File not found: ...`, qui lui parle de
     « racine inventée ».
- Défaut : (1) le modèle ne sait pas laquelle des occurrences il visait ni quel contexte les
  distingue. (2) La cause la plus courante est une lecture périmée (le modèle a déjà modifié
  cette zone et recopie l'ancien texte) ou le mauvais fichier : une ligne proche ne le dit
  pas. Deux modifications ratées du même fichier déclenchent la détection de blocage, donc
  chaque échec évité compte. (3) et (4) sont des appels dont l'intention est claire.
- Remède : joindre l'information au lieu de renvoyer lire.
  1. Ambigu : donner les occurrences avec leur ligne précédente :
     `[Claw] edit_file changed nothing: old_string appears 3 times, at lines 12, 48 and 103. Line before each: 11 "def load(path):", 47 "def save(path):", 102 "def reset():". Add that line to old_string to pick one, or set replace_all to true to change all three.`
  2. Introuvable : joindre les lignes réelles du fichier autour de la ligne la plus proche
     (3 avant, la longueur de `old_string`, 3 après, 40 lignes au plus), numérotées :
     `[Claw] edit_file changed nothing: old_string is not in app.py. The closest place is lines 40 to 52, shown below exactly as they are now. Copy old_string from these lines.`
     Si le texte cherché est dans le contenu d'avant une modification de ce tour (les
     originaux sont déjà gardés pour la relecture) : `This text was there before your previous
     edit of this file; the lines below are the current ones.` Sans ligne proche, chercher la
     première ligne dans le projet et le dire : `It is not in app.py, but "def total(" is at
     src/panier.py:17. Did you mean that file?`
     Niveaux de tolérance à ajouter avant l'échec, chacun signalé dans `matchNote` : `\n` et
     `\t` écrits en deux caractères, guillemets typographiques contre guillemets droits,
     espace insécable.
  3. Chaînes identiques : `[Claw] edit_file changed nothing: old_string and new_string are identical, and this text is already in the file at line 12. If that is the result you wanted, go on; otherwise put the changed text in new_string.`
  4. `old_string` vide et fichier absent ou vide : reprise, créer le fichier avec
     `new_string` et le dire (`Created app.py with new_string, as write_file would`). Fichier
     existant non vide : garder l'erreur actuelle.
  Aucune requête de plus ; chaque cas en économise une.
- État : corrigé (lot 3) : occurrences multiples avec leurs lignes et la ligne d'avant, texte introuvable avec les vraies lignes autour de la plus proche (numérotées), textes identiques avec la ligne où le texte est déjà, old_string vide sur un fichier absent ou vide : fichier créé. Tolérances ajoutées : guillemets échappés, \n en deux caractères, espace insécable. Pas fait : texte d'avant une modification du même tour, recherche du texte dans d'autres fichiers.

## E1-19 write_file et edit_file : un contenu abrégé (« ... rest of the code unchanged ... ») est écrit tel quel
- Gravité : haute
- Où : rust/crates/runtime/src/file_ops.rs:404 à 450 (`write_file`), :452 à 507 (`edit_file`)
- Aujourd'hui : `write_file` écrit `content` sans regarder ce qu'il remplace
  (`fs::write(&absolute_path, bytes)?`) et rend `"type": "update"`. Aucun contrôle ne cherche
  une ligne d'élision dans `content` ou `new_string` (recherche de « existing code », « rest
  of », « unchanged », « remains the same » dans les caisses : rien).
- Défaut : un petit modèle qui « réécrit » un fichier de 400 lignes en envoie 60, dont
  `# ... (rest of the code remains unchanged)` ou `// ... existing code ...`. Le fichier est
  remplacé, 340 lignes sont perdues, le résultat est un succès. Un commentaire ne casse pas la
  syntaxe : le contrôle de syntaxe ne voit rien, et la vérification du projet échoue plus
  tard sur des symboles absents que le modèle ne relie pas à son écriture. C'est l'erreur
  silencieuse la plus destructrice des outils de fichiers.
- Remède : contrôle par Claw avant d'écrire, sans requête de plus. Refuser quand les deux
  conditions sont réunies : (a) une ligne de `content` (ou de `new_string` absente de
  `old_string`) est un commentaire d'élision, reconnu par un motif du genre
  `^\s*(#|//|/\*|<!--|--|;)?\s*(\.\.\.|…).{0,60}(rest|remaining|existing|previous|unchanged|same as|omitted|reste|inchang)`
  ou l'inverse (`existing code ...`) ; (b) pour `write_file`, le fichier existe et le nouveau
  contenu a moins de la moitié de ses lignes. Message :
  `[Claw] write_file was not executed: line 14 of your content is a placeholder ("# ... rest of the code remains unchanged"), not code, and the file would shrink from 412 to 58 lines. write_file replaces the whole file with exactly what you send. To change part of app.py, use edit_file with the lines that change; to rewrite it, send every line.`
  Sans la condition (b), écrire mais joindre un avertissement. Coupé par
  `CLAW_PLACEHOLDER_GUARD=0`.
- État : corrigé (lot 3) : is_elision_line et placeholder_check ; write_file refusé quand le fichier fondrait de plus de moitié, sinon écrit avec une note ; edit_file refusé quand une élision remplace des lignes. Aide CLAW_PLACEHOLDER_GUARD.

## E1-20 bash : sans shell POSIX le modèle reçoit une consigne d'installation, et une commande introuvable ne reçoit aucune aide
- Gravité : moyenne
- Où : rust/crates/runtime/src/bash.rs:66 à 71 (`execute_bash`), rust/crates/plugins/src/shell.rs:89 à 92 (`missing_shell_message`), rust/crates/claw-cli/src/main.rs:948 à 960 (choix des outils), rust/crates/runtime/src/conversation.rs:1177 à 1201 (`after_tool`)
- Aujourd'hui :
  1. Sous Windows sans Git Bash, chaque appel `bash` rend
     `aucun shell POSIX trouvé : installez Git for Windows (https://git-scm.com/download/win) ou définissez CLAW_SHELL avec le chemin de bash.exe`.
     L'outil `bash` reste pourtant proposé (`_ => true`) à côté de `PowerShell`
     (`"PowerShell" => !posix_shell`).
  2. Une commande qui échoue rend sa sortie et `"returnCodeInterpretation": "exit_code:127"`.
     Claw y joint le code des emplacements cités, la remarque sur les chemins `dossier\fichier`
     et celle sur un outil écrit comme une commande. Rien pour `command not found` (aucune
     occurrence dans le code hors d'un test), ni pour `No such file or directory` sur un `cd`.
- Défaut : (1) s'adresse à l'utilisateur. Le modèle ne peut rien installer et n'est pas
  renvoyé vers `PowerShell` : il réessaie `bash` ou s'arrête. (2) Sous Git Bash, `python3` et
  `make` manquent souvent, les commandes internes de cmd (`copy`, `del`, `move`, `cls`)
  n'existent pas, et `type fichier` est une autre commande (elle décrit un nom de commande au
  lieu d'afficher un fichier). Le modèle essaie des variantes une requête à la fois, ou dit à
  l'utilisateur d'installer un outil qui existe sous un autre nom.
- Remède :
  1. Sans shell POSIX : ne pas proposer `bash`, et si le modèle l'appelle quand même :
     `[Claw] The bash tool is not available on this machine (no POSIX shell). Run commands with the PowerShell tool instead, in PowerShell syntax.`
     Prévenir l'utilisateur une fois, dans l'interface, que Git for Windows rend `bash`
     disponible.
  2. Code 127 ou « command not found » : chercher soi-même les équivalents avec
     `runtime::find_executable` et les joindre :
     `[Claw] "python3" is not installed here. Found instead: python (C:/Python312/python.exe), py. Run the command again with one of them.`
     Table courte : `python3`/`python`/`py` ; `pip3`/`pip`/`python -m pip` ; `node`/`nodejs` ;
     commandes de cmd vers leur équivalent POSIX (`copy` vers `cp`, `del` vers `rm`, `move`
     vers `mv`, `cls` vers `clear`, `type fichier` vers `cat fichier`). Rien de trouvé :
     `[Claw] "make" is not installed on this machine and no equivalent was found. Do not retry it: look for another way (the commands of the Makefile themselves), or tell the user it must be installed.`
     `cd: X: No such file or directory` : joindre le dossier de travail et les dossiers au nom
     proche, comme `missing_path_message` le fait pour les fichiers.
  Aucune requête de plus.
- État : à vérifier

## E1-21 Valeurs hors bornes acceptées telles quelles : `limit: 0`, `Sleep` sans plafond, délai de `REPL` ignoré
- Gravité : moyenne
- Où : rust/crates/runtime/src/file_ops.rs:365 à 387 (`read_file`), rust/crates/tools/src/lib.rs:3910 à 3916 (`execute_sleep`), :4019 à 4030 (`execute_repl`)
- Aujourd'hui : les schémas donnent des bornes (`"limit": { "minimum": 1 }`) que le code ne
  contrôle pas.
  1. `read_file` avec `limit: 0` (« sans limite » pour beaucoup de modèles) : `end_index`
     vaut `start_index`, le contenu est vide, et la note dit
     `Showing lines 1-0 of 120. Call read_file again with offset 0 to read on, ...` : le
     conseil ramène au même appel.
  2. `read_file` d'un fichier vide : `content: ""`, `numLines: 0`, `totalLines: 0`, sans
     note.
  3. `Sleep` : `std::thread::sleep(Duration::from_millis(input.duration_ms))`, sans plafond
     et sans regarder l'interruption. `duration_ms: 5` (un modèle qui pense en secondes) dort
     5 ms et rend `Slept for 5ms` ; `duration_ms: 3600000` bloque le tour une heure sans que
     l'utilisateur puisse l'arrêter.
  4. `REPL` : `let _ = input.timeout_ms;` puis `.output()` : le délai donné par le modèle
     n'est pas appliqué, un code qui attend une saisie ou boucle bloque le tour.
- Défaut : (1) et (2) font croire à un fichier vide ou à une lecture ratée. (3) et (4) sont
  des blocages que ni le modèle ni l'utilisateur ne peuvent lever, alors que `bash` a déjà
  tout ce qu'il faut (délai lu en secondes sous 1 000, interruption, arrière-plan).
- Remède : ramener dans les bornes et le dire.
  1. `limit: 0` vaut la limite par défaut. Note : `[Claw] "limit": 0 was read as the default (2000 lines).`
  2. Fichier vide : `[Claw] This file exists and is empty (0 bytes).`
  3. `Sleep` : valeur sous 1 000 lue en secondes (comme `effective_timeout_ms` le fait pour
     `bash`), plafond de 5 minutes, attente par tranches qui regardent l'interruption.
     Note quand la valeur a été changée : `[Claw] Waited 300 s, the longest Sleep allows (you asked for 3600 s). To wait for a command, use BashOutput with "block": true instead.`
  4. `REPL` : passer par `runtime::run_tool_command`, comme `PowerShell`, pour appliquer le
     délai et l'interruption.
  Aucune requête de plus.
- État : en partie (lot 3) : limit 0 lu comme la limite par défaut, fichier vide dit. Restent Sleep (plafond, interruption) et REPL (délai).

## E1-22 Chemin introuvable : le message suppose une « racine inventée », ne propose aucun dossier et ne dit pas comment créer
- Gravité : moyenne
- Où : rust/crates/runtime/src/file_ops.rs:1341 à 1388 (`similar_files`), :1392 à 1406 (`missing_path_message`), :1408 à 1425 (`normalize_path`)
- Aujourd'hui : pour tout chemin absent, quel que soit l'outil :
  `File not found: {path}` / `The working directory is {cwd}. Use a path relative to it (for
  example `src/main.rs`), not an invented root.` / `Files with a similar name: ...`.
  `similar_files` ne retient que des fichiers (`if entry_path.is_dir() { ...; continue; }`).
  Les autres erreurs de `canonicalize` (`Err(error) => Err(error)`) passent brutes, sans le
  chemin : nom invalide sous Windows (`os error 123`), accès refusé.
- Défaut : trois cas mal servis. (a) Chemin relatif correct d'un fichier qui n'existe pas
  encore (`edit_file` ou `read_file` sur un fichier que le modèle croit avoir créé) : le
  conseil sur la racine inventée égare, et rien ne dit de le créer avec `write_file`.
  (b) Dossier donné à `glob_search` ou `grep_search` (`path: "source"` pour `src`) : aucun
  dossier proposé, et le texte dit « File ». (c) Faute dans un dossier intermédiaire
  (`src/componets/Button.tsx`) : seuls les fichiers nommés `Button*` sont cherchés, ce qui
  marche, mais le modèle ne voit pas quel segment est faux.
- Remède : joindre ce que Claw sait déjà, sans requête de plus.
  1. Dire jusqu'où le chemin existe, et montrer le contenu du dernier dossier trouvé :
     `[Claw] src/componets/Button.tsx does not exist: src exists, but it has no folder "componets". src contains: components/, hooks/, pages/, App.tsx. Closest match: src/components/Button.tsx.`
  2. Ne garder la phrase sur la racine inventée que si le chemin est absolu ou commence par
     un dossier absent du projet.
  3. Selon l'outil : pour `edit_file`, `To create this file, call write_file with its full
     content.` ; pour `glob_search` et `grep_search`, proposer les dossiers au nom proche et
     dire `Folder not found`.
  4. Autres erreurs : nommer le chemin et la cause (`"a<b.txt" is not a valid file name on
     Windows: remove < > : " | ? *`).
- État : corrigé (lot 3) : le message dit jusqu'où le chemin existe et ce que contient le dernier dossier trouvé ; la phrase sur la racine inventée seulement hors du projet ; edit_file ajoute comment créer le fichier.

## E1-23 BashOutput et KillShell : identifiant voisin refusé, et tâche perdue après un redémarrage sans explication
- Gravité : basse
- Où : rust/crates/runtime/src/bash_tasks.rs:74 à 78 (`find`), :427 à 433 (`unknown_task`), rust/crates/tools/src/lib.rs:1376 à 1396
- Aujourd'hui : `find` n'accepte que l'identifiant exact (`bash_3`) ou le PID. `3`, `bash-3`,
  `bash3`, `shell_3`, `task_3` rendent `unknown background task `3` (known: bash_3)`. Les
  tâches vivent en mémoire : après un redémarrage de Claw ou une reprise de session, le modèle
  qui lit `bash_3` dans la conversation reçoit
  `unknown background task `bash_3` (no background task is running)`.
- Défaut : le premier cas coûte une requête pour une intention sans ambiguïté. Le second ne
  dit pas pourquoi la tâche a disparu : le modèle croit le serveur toujours lancé et donne son
  adresse à l'utilisateur (voir A4 du suivi).
- Remède :
  1. Reprise : lire `3`, `bash-3`, `bash3`, `shell_3`, `task_3` comme `bash_3` ; pour
     `BashOutput` seulement, s'il n'existe qu'une tâche et que l'identifiant est inconnu,
     lire celle-là et le dire.
  2. `[Claw] There is no background command "bash_3" any more: background commands stop when Claw restarts, and none is running now. If you still need it (a server, a watcher), start it again with bash and "run_in_background": true.`
  Aucune requête de plus.
- État : à vérifier

## E1-24 Hooks : un refus après exécution marque en erreur un outil qui a réussi, parfois sans un mot
- Gravité : basse
- Où : rust/crates/runtime/src/conversation.rs:1925 à 1938 et :1082 à 1088 (`PostToolUse`), :1908 à 1915 et :1016 à 1020 (`PreToolUse`), :2142 à 2158 (`merge_hook_feedback`)
- Aujourd'hui : après l'exécution, `if post_hook_result.is_denied() { is_error = true; }` puis
  `merge_hook_feedback(messages, output, true)`, qui ajoute `Retour des hooks (refusé):` et les
  messages du hook. Si le hook n'a rien écrit, `if messages.is_empty() { return output; }` :
  le résultat de réussite de l'outil part tel quel, marqué en erreur. Avant l'exécution, un
  hook qui refuse sans message donne `hook PreToolUse : outil `edit_file` refusé`.
- Défaut : « refusé » après coup fait croire que la modification n'a pas eu lieu, alors que
  le fichier est écrit : le modèle la refait, et reçoit « old_string not found ». Sans message,
  il voit un résultat normal signalé comme une erreur et ne peut rien en déduire. Le refus
  avant exécution ne dit ni pourquoi ni quoi faire.
- Remède : textes qui disent ce qui s'est passé.
  `[Claw] The tool ran and its result is above: the change is applied. A PostToolUse hook of this project then reported a problem: <messages>. Fix what it reports; do not repeat the call.`
  Sans message du hook : `... then failed without saying why (exit code 2). Tell the user; do not repeat the call.`
  Avant exécution, sans message : `[Claw] edit_file was not executed: a PreToolUse hook of this project blocked it without giving a reason. Do not retry it; continue with the rest and tell the user which call was blocked.`
  Aucune requête de plus.
- État : à vérifier

## E1-25 Textes en français rendus au modèle, souvent écrits pour l'utilisateur
- Gravité : basse
- Où : liste ci-dessous
- Aujourd'hui : la consigne (prompt.rs:510) annonce au modèle des résultats d'outils et des
  remarques `[Claw]` en anglais. Ces résultats d'outils lui arrivent pourtant en français :
  - permissions.rs:248 `l'outil '{tool_name}' est interdit par la règle de permission `{}`` ; :273 et :305 (voir E1-17) ;
  - claw-cli/src/main.rs:5138 `l'utilisateur a refusé l'outil '{}' ; attendez sa consigne avant de continuer`, :5190, :5196, :5124 et :5179 `échec de la demande d'approbation : {error}`, :6990 `l'outil `{tool_name}` n'est pas activé par l'option --allowedTools actuelle` ;
  - claw-cli/src/web.rs:528, stream_json.rs:134, acp.rs:328 et :331 (refus et absence de réponse) ;
  - runtime/src/interrupt.rs:11 `tour interrompu par l'utilisateur` ; bash.rs:358 (première ligne du dépassement de délai) et :369 `Commande interrompue par l'utilisateur` ;
  - runtime/src/conversation.rs:1018 et :1910 (hook), :2152 à 2154 `Retour des hooks`, :2114 `[… sortie tronquée lors de la compaction du contexte ; relancez l'outil avec une portée plus étroite si besoin]` ;
  - tools/src/lib.rs:2350 `le sous-agent `{}` a échoué : {error}` ; mcp_bridge.rs:468 `aucun serveur MCP n'est connecté`, :477 `outil MCP inconnu : {name}`, :492, :498, :502 ;
  - plugins/src/shell.rs:90 (voir E1-20) ; lib.rs:1777 `SearXNG : {error} (activez le format json dans settings.yml)` ;
  - runtime/src/file_ops.rs:488, :587, :595, :599 et :605 (notes `matchNote` de `edit_file` : `correspondance approximative : indentation ajustée à celle du fichier`).
- Défaut : un petit modèle suit mieux une consigne dans la langue de ses autres consignes, et
  plusieurs de ces textes disent à l'utilisateur quoi régler (`--allowedTools`,
  `settings.yml`) au lieu de dire au modèle quoi faire ensuite. Le même texte sert souvent à
  l'interface et au modèle : c'est la cause.
- Remède : séparer les deux destinataires. Garder le texte français pour l'affichage
  (`skipped`, `notify`) et rendre au modèle un texte anglais préfixé `[Claw]` qui finit par
  la suite à donner, par exemple :
  `[Claw] "Config" is not enabled in this session, so nothing was executed. Do not call it again; continue with the other tools.`
  `[Claw] The MCP tool "mcp__github__search" did not answer within 60 s. It may be slow or down: try once more, then continue without it and say so.`
  Aucune requête de plus.
- État : à vérifier

## E1-26 Skill, EditorDiagnostics, Config : un nom faux ne reçoit pas la liste des noms justes, ou reçoit « rien à signaler »
- Gravité : basse
- Où : rust/crates/tools/src/lib.rs:2207 à 2268 (`resolve_skill_path`), :1035 à 1080 (`run_editor_diagnostics`), :3970 à 3980 (`execute_config`), rust/editors/vscode/extension.js:463 à 469
- Aujourd'hui :
  1. `Skill` : `unknown skill: {requested} (searched .claw/skills, .claude/skills and
     .codex/skills in the project and home directory, the skills synced by the Claude app and
     those of enabled Claude Code plugins)`. Les dossiers sont parcourus, mais les noms trouvés
     ne sont pas rendus.
  2. `EditorDiagnostics` avec un `path` faux ou jamais ouvert : l'extension rend
     `vscode.languages.getDiagnostics(uri)`, vide pour un fichier inconnu, et le modèle reçoit
     `"errors": 0`, `"problems": ["none reported"]`. Rien ne contrôle que le fichier existe.
  3. `Config` : `"success": false`, `"error": "Unknown setting: \"x\""`, sans la liste des
     réglages connus (elle est dans `supported_config_setting`).
- Défaut : (1) et (3) laissent le modèle deviner un autre nom. (2) est une erreur silencieuse :
  « aucun problème » pour un fichier qui n'existe pas, ou que l'éditeur n'a pas analysé, passe
  pour une vérification réussie.
- Remède :
  1. `[Claw] There is no skill named "deploy". Skills available here: release-notes, db-migration. If none fits, do the task with your other tools.` (ou `No skill is installed for this project`).
  2. Contrôler le chemin avec `normalize_path` avant d'interroger l'éditeur (erreur et
     fichiers proches s'il est absent). Quand rien n'est signalé, le dire sans laisser croire
     à une preuve :
     `[Claw] The editor reports nothing for src/app.ts. That is not proof the file is correct: an editor only checks files its language extension has analysed. Run the build or the tests to be sure.`
  3. Joindre la liste : `Known settings: theme, editorMode, verbose, model, language, ...`.
  Aucune requête de plus.
- État : à vérifier

## Examiné sans constat

Pour mémoire, lus et jugés corrects (le message dit quoi faire) : `FindSymbol`, `Remember`,
`ExitPlanMode`, `ViewPage` (page_view.rs:228 à 246), les trois messages d'appel coupé de
`input_check::problem`, les remarques de `bash` sur l'arrière-plan et le délai (bash.rs:276 à
365), la note de fin de lecture de `read_file`, le refus de codage de `edit_file`
(file_ops.rs:476 à 486), les fichiers binaires et trop gros (`read_text`). De
`AskUserQuestion` je n'ai lu que les messages d'erreur (questions.rs:104 à 165) : ils sont
complets. `Consult` : seul bémol, `set "advisorModels" in the settings` s'adresse à
l'utilisateur.

Non étudié : les erreurs des outils de plugins (`plugin_tools`), le détail des erreurs HTTP
des serveurs MCP, et `PowerShell` au-delà de `detect_powershell_shell`.

## Ordre de traitement conseillé

Les constats sont dans l'ordre où ils ont été confirmés. Par gain pour un petit modèle :

1. E1-19 (contenu abrégé écrit tel quel) : seule perte de données silencieuse sur du code.
2. E1-07 et E1-10 (recherches vides sans explication) : faussent tout le raisonnement qui suit.
3. E1-01, E1-04 et E1-11 : une même couche de « mise en forme des arguments d'après le
   schéma » (types voisins, noms voisins, tableau reçu comme texte), à placer dans
   `input_check.rs` et à appeler des deux exécuteurs (`CliToolExecutor::run_tool`,
   `SubagentToolExecutor::execute`), qui rend les arguments réparés et les notes à joindre.
4. E1-02 et E1-03 (outil inconnu), E1-05 et E1-06 (dossier lu, motif non valide) : reprises
   courtes à écrire.
5. E1-18 (messages de `edit_file`), E1-08 et E1-09 (mode texte et JSON).
6. E1-12 et E1-13 (sous-agents), E1-15 et E1-16 (web), E1-17 (lecture seule).
7. Le reste : E1-14, E1-20 à E1-26.

