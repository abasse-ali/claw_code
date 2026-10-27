# C1. Rivaliser avec Claude : la session #35 (appli de budget)

Point d'entrée de toute session qui reprend ce chantier. Il dit ce qui a été décidé, ce qui
est fait, ce qui reste, et comment vérifier. À tenir à jour à chaque lot, dans le même commit.

## La demande

Le 27 octobre 2026, l'utilisateur a lancé le même prompt sur Claude Code (Opus 5.5, réflexion
moyenne) et sur Claw (session #35 de `claw-sessions`, modèle
`nvidia:nvidia/nemotron-3-ultra-550b-a55b`, branche `claw/session-9` de `budget_claw`) :

> Tous les mois je reçois : + salaire = 1235.15 euro, + mobili-jeune = 100 euro, + caf = 103
> à 206 euro, ~ espece = 80 à 200 euro ; dépenses impossibles à éviter : - loyer = 450 euro,
> - forfait = 10 euro, - claude = 21.60 euro, - transport = 8 euro (normalement 16, Orange
> rembourse 50 %), - alimentaire = 100 euro. « Je veux une appli web perso utilisable sur
> téléphone incluant toutes les fonctionnalités dont j'ai besoin, et que ce soit beau. »
> (`+` gain, `-` perte, `~` gain non assuré.)

| | Claude | Claw (#35) |
|---|---|---|
| Durée | 10 min (12:16 à 12:26) | 44 s (questions) puis 109 min, arrêté par l'heure |
| Requêtes, coût | 2,46 $ | 305 requêtes gratuites |
| Résultat | montants déjà remplis, `~` et fourchette de la CAF gérés, testé sur iPhone simulé en clair et en sombre, hors-ligne | appli vide (0,00 € partout), texte tapé noir sur noir, graphique de l'historique vide (canvas 0x0), salaire recopié sur les 11 mois d'avant, mode clair imposé aux téléphones réglés en clair malgré « Sobre foncé » |
| Compte rendu | juste | « fait et vérifié » faux (montants, graphique), lien de test `https://[votre-domaine]` |

Puis : « Arrange tout, je te donne carte blanche, et fais aussi gagner en vitesse de
traitement (diminution des délais de "Claw travaille") ».

## Décisions de l'utilisateur (27 octobre 2026)

- **Essais réels** : oui, **2 passages au plus**, avec le prompt du budget, comparés à #35.
- **Livraison** : **fusion directe dans `main`** quand tout est vérifié (les sessions de Claw
  Web utilisent `main`).
- **Modèle payant** : non, **gratuit uniquement** (aucun « modèle fort » payant, même en option).
- **Vitesse** : **couper le bruit, garder les barres à 8** (fausses alertes, messages redits,
  vérifications regroupées ; la qualité exigée ne baisse pas).

## Diagnostic du passage #35 (journal décodé de l'issue #35)

Le job ne coûte que 16 s ; « Claw travaille » dure parce que l'agent travaille : 304 requêtes
(médiane 9 s, moyenne 21 s ; l'écriture de `app.js` seule : 9 min avec une reprise), 141
commandes `bash`, 67 modifications, 35 lectures. De 17:00 à 18:24, presque tout a porté sur
des défauts mal mesurés :

1. **Mauvais mode** : la page était mesurée sans `theme`, donc en clair ; les contrastes à
   1,1:1 venaient du jaune `#ffe8a0` sur le beige du mode clair, et l'agent a changé une
   dizaine de fois les couleurs du mode sombre (fond jusqu'à `#000000`).
2. **`file://`** : « Unsafe attempt to load URL file:///… » (le manifeste) compté « erreur du
   script », fonctionnement plafonné à 5/10, puis « sous la barre 8 : le tour continue ».
3. **Menu laissé ouvert** par l'essai du menu au téléphone (`screenshot_probe.js`) : chaque
   bouton testé ensuite était « impossible à toucher » (sous le fond du menu).
4. **« Bouton de menu déjà là »** alors que le tiroir était hors de l'écran (x = -260) :
   l'audit prenait toute boîte de taille non nulle pour visible.
5. **« Importer » qui « ne fait rien »** : un sélecteur de fichier, invisible sans écran.
6. **Vrais bugs jamais mesurés** : montants absents, texte tapé illisible, graphique vide.
7. **Contrôle des exigences** seulement à la conclusion, et sauté quand le temps manque
   (`conversation.rs` : « la conclusion part telle quelle ») : d'où les « fait et vérifié »
   faux.

## Lots

| Lot | Contenu | État |
|---|---|---|
| 1. Mesures justes | Pages du projet servies par un petit serveur local en `http://127.0.0.1:<port>` (`tools/src/local_server.rs`, `page_url` dans Screenshot, Browser et ViewPage ; `CLAW_PAGE_SERVER=0` revient au `file://`) ; caches du service worker vidés avant chaque ouverture dans Browser ; `favicon.ico` absent servi en 204 ; filtre de « Unsafe attempt to load URL file: » ; mode clair par défaut, et l'autre mode mesuré aussi quand la page suit `prefers-color-scheme` (défauts de couleur préfixés « In light mode » / « In dark mode », avec leurs règles CSS) ; mode demandé par l'utilisateur (`requested_theme` : « sombre », « foncé », « dark »…) passé aux captures par le runtime | fait |
| 2. Fausses alertes | Menu refermé après son essai (sinon page rouverte) ; menu « déjà là » seulement s'il est dans l'écran ; sélecteur de fichier compté comme une action ; liens d'une page servie vérifiés sur le disque (« which does not exist ») ; composant carrousel : points cachés quand toutes les images tiennent à l'écran (défaut ancien, que l'erreur `file://` masquait dans son essai) | fait |
| 3. Vrais bugs mesurés | Audit : texte tapé illisible (7a), graphiques vides (7b), valeurs `Infinity` et `Invalid Date` ; chiffres de la demande (`request_figures` dans le runtime, entrée `figures` de Screenshot, `missing_figures`) ; écrans d'une appli d'une seule page ouverts et mesurés (`VIEWS_SCRIPT`, sorte `view`) ; note de fonctionnement (`FUNCTION_FAULTS` : chiffres absents, texte tapé illisible, graphique vide) | fait |
| 4. Le fond avant la forme | Première version complète d'une page créée : confrontée à la demande avant les retouches (`early_requirements`, `early_requirements_note`) ; vérification après modification réduite à une ligne quand rien n'a bougé, conseil de recul à la troisième (`unchanged_check_feedback`, `check_listed`) ; tour arrêté par sa limite : Claw ajoute à la réponse ce qu'il a pu vérifier (`wrap_up_status`, `append_to_answer`) ; « appli web », « appli mobile », « PWA » et « je veux… » reconnus comme des demandes d'interface (la demande de #35 ne l'était que par les notes du moteur) | fait (trois tests de conversation) |
| 5. Parcours en un appel | `Browser` avec `"action": "steps"` et `steps` : jusqu'à 40 actions jouées d'un appel, la page posée après chacune, arrêt à la première qui échoue ; `expect` et `absent` sur chaque étape (et sur une action seule) vérifient le résultat (un total, un nom, « NaN » absent) ; la méthode de design demande le parcours entier en un appel avec ses résultats attendus ; progression de l'appli : « Navigateur : parcours de N étapes » (24 appels `Browser` dans #35) | fait |
| 6. Kit d'appli | Cinq composants de l'outil `Component` pour une appli perso : `app-tabs` (écrans d'une seule page derrière une barre d'onglets, en bas au téléphone ; l'écran suit l'adresse, `screenchange` à chaque changement ; les messages passent au-dessus de la barre), `entries` (données gardées sur le téléphone ; les chiffres de l'utilisateur posés au mois en cours dès le premier lancement, jamais recopiés sur les mois d'avant ; liste du mois et totaux ; suppression avec « Annuler » ; copie en fichier tableur `;` et virgule décimale, reprise sans doublon grâce à l'identifiant), `entry-sheet` (volet d'ajout qui monte du bas, montant « 12,50 » ou « 12.50 », champs lisibles en sombre), `chart` (barres en SVG à la largeur de leur écran, redessinées à son ouverture, sans boucle de `ResizeObserver`), `pwa` (balises du `<head>`, manifeste à `start_url` relative, service worker réseau d'abord, rendus sous `files`). `toast` reçoit une action (« Annuler »). Nouvel outil `AppIcon` (`tools/src/app_icons.rs`) : le navigateur dessine les PNG 192, 512, maskable et 180 (iPhone) depuis l'icône SVG, refuse une icône vide ; fond pris au manifeste. La méthode de design demande le kit, les cinq ensemble, pour une appli qui garde les données de l'utilisateur, et `AppIcon` pour les PNG | fait |
| 7. Vitesse | Mesuré sur le journal de #35 (temps entre deux requêtes, selon ce qui s'y passe) : les 62 vérifications après modification pesaient le plus. Chacune durait 29 s sur l'appli de #35, dont 18 s pour rouvrir la page à chaque vue (fenêtres, menu, écrans), contre 6 à 9 s pour la requête suivante du modèle : de l'ordre de 20 min d'attente sur 109. Changements, barres et mesures inchangées (mêmes défauts trouvés, comparés avant et après sur l'appli de #35 et sur celle du kit) : (1) les vues sont relevées pendant la capture du téléphone, sans rouvrir la page, et une vue se rouvre sans faire défiler toute la page (`open_page_with`) : 29 s → 20 s (appli de #35), 17 s → 10 s (kit) ; (2) la requête suivante n'attend plus une vérification encore en route que 1,5 s (`CHECK_GRACE`, `ready_visual_check`) : son résultat suit au pas d'après, et les modifications faites pendant ce temps sont regroupées dans la vérification suivante (« vérifications regroupées »). Les revues de début et de fin de tour attendent toujours la leur. Fausse alerte trouvée en mesurant, corrigée : le parcours tapait son nom d'essai dans un montant (`inputmode="decimal"`) et disait l'opération « montrée nulle part » ; un champ numérique reçoit un nombre, et un nom ajouté à un écran caché qu'un lien ou un onglet ouvre compte comme montré | fait |
| 8. Banc t9-budget | `rust/scripts/real-tests/scenarios/t9-budget` : la demande de #35 avec les réponses à ses questions, dossier vide ; `check.py` sert le projet en http et le mène dans un vrai navigateur par DevTools (client WebSocket en Python sans dépendance), 12 points (vérifications cachées ci-dessous, plus une dépense gardée après un rechargement). Étalonné : l'appli de Claude 11/12 (seul le mode sombre manque : elle n'avait pas la réponse « sobre foncé »), celle de #35 4/12 | fait |
| 9. Livraison | Vérification complète, essai réel (2 passages au plus), comparaison avec #35, **fusion directe dans `main`** | à faire |

Vérifications cachées prévues pour t9 (tirées de la comparaison) : montants présents au
premier lancement (`1235.15`, `103`, `206`, `80`, `21.6`) ; espèces comptées seulement une
fois reçues ; fourchette de la CAF (prudent et au mieux) ; mode sombre respecté quand il est
demandé ; texte tapé lisible ; graphique dessiné ; historique non inventé (mois d'avant
vides) ; aucune erreur de console en `http` ; manifeste à `start_url` relative et service
worker (hors-ligne) ; aucun débordement horizontal au téléphone.

## Suite, dans l'ordre

1. Lot 9.
2. Lot 9 : essai réel, puis fusion dans `main`, puis mise à jour de ce fichier et de
   `rust/docs/suivi-intelligence.md`.

## Vérifier

Depuis `rust/` :

```bash
cargo fmt --all
cargo clippy --workspace --all-targets            # zéro avertissement
CLAW_NO_USER_ENV=1 cargo test --workspace -- --test-threads=1
# Essais avec un vrai navigateur (Chromium, Chrome ou Edge) :
CLAW_TEST_BROWSER=1 CLAW_BROWSER=/chemin/vers/chrome CLAW_NO_USER_ENV=1 \
  cargo test -p tools -- --ignored --test-threads=1
```

- Dans un conteneur où l'on est `root`, Chromium refuse de démarrer sans `--no-sandbox` :
  `CLAW_BROWSER` vise alors un petit script `#!/bin/sh` qui lance
  `exec /chemin/vers/chrome --no-sandbox "$@"`.
- `advisors::tests::live_advisor_diagnoses_a_stuck_session` échoue sans `CLAW_LIVE_ADVISORS`
  (il appelle un vrai modèle) : c'est attendu.
- `hooks::tests::collects_and_runs_hooks_from_enabled_plugins` (crate `plugins`) peut échouer
  une fois sur « Broken pipe » et passer au lancement suivant (vu le 27 octobre 2026, avant
  ces changements).
- Le test du navigateur qui rejoue la session #35 :
  `cargo test -p tools -- --ignored the_budget_app_of_session_35`.

Essais du lot 6 avec un vrai navigateur : `every_component_works_in_a_real_browser` (les 23
composants hors `pwa` dans une page), `png_icons_are_drawn_from_the_svg_in_a_real_browser`, et
`the_app_kit_makes_a_budget_app_that_works_in_a_real_browser` : l'appli de budget de #35 bâtie
avec le kit, en sombre, mesurée par Screenshot (fenêtres et exploration, chiffres de la demande)
puis parcourue par Browser (ajout de 12,50, liste, suppression, « Annuler », graphique). Cet
essai a trouvé deux défauts du kit, corrigés : le message de confirmation couvrait la barre
d'onglets du téléphone, et la balise `apple-mobile-web-app-capable` est signalée comme dépassée.

Points de mesure le 27 octobre 2026 : sans ces changements, 956 tests passent ; avec un vrai
navigateur, 37 sur 39 passent (`advisors`, attendu, et `components`, qui échouait justement
sur « Unsafe attempt to load URL file:// »). Avec les lots 1 à 3 : 963 tests passent (966 avec les tests du lot 4) ; avec
un vrai navigateur, 39 sur 40 (seul `advisors`, attendu), dont le nouvel essai de la session
#35.

`cargo clippy` (version 1.97) signale des avertissements anciens dans des fichiers que ce
chantier n'a pas touchés, et des fonctions déjà trop longues avant lui (`take`, `capture` de
`screenshot.rs`, plusieurs de `tools/src/lib.rs`) : n'en ajouter aucun dans le code modifié
(`--message-format=short`, filtré sur les fichiers du lot).

## Essai réel (lot 9)

Accord de l'utilisateur : 2 passages au plus (le passage des questions compte). Pour essayer
la branche sans toucher `main` :

1. Dans `abasse-ali/claw-sessions`, sur une branche, faire pointer le workflow
   `.github/workflows/claw-web.yml` sur `abasse-ali/claw_code/.github/workflows/claw-web.yml@<branche>`
   et lui passer `claw_ref: <branche>`.
2. Créer la session comme l'appli : une issue étiquetée `claw-web`, le prompt du budget, et
   en fin de corps `<!-- claw-web {"repo":"abasse-ali/budget_claw","base":"main","model":"nvidia:nvidia/nemotron-3-ultra-550b-a55b","n":<n>,"branch":"claw/session-<n>"} -->`.
3. Lancer le workflow sur cette branche (`repo`, `base`, `session` = numéro de l'issue,
   `model`, `runner` = `claw-pc` comme #35). Répondre aux questions comme #35 (« Fonctions :
   Tout (Recommandé) », « Style : Sobre foncé »), puis relancer.
4. Comparer avec #35 : durée, requêtes, vérifications cachées ci-dessus.
