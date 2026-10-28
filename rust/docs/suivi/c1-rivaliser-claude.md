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
| 9. Livraison | Vérification complète (969 tests, 42 essais navigateur sur 43 : seul `advisors`, attendu), essai réel en un passage (session #36, ci-dessous), corrections tirées du passage (parcours Browser écrits autrement, entrée absente de sa liste, champs illisibles de l'autre mode dans les écrans cachés), **fusion directe dans `main`** le 27 octobre 2026 | fait |

Vérifications cachées prévues pour t9 (tirées de la comparaison) : montants présents au
premier lancement (`1235.15`, `103`, `206`, `80`, `21.6`) ; espèces comptées seulement une
fois reçues ; fourchette de la CAF (prudent et au mieux) ; mode sombre respecté quand il est
demandé ; texte tapé lisible ; graphique dessiné ; historique non inventé (mois d'avant
vides) ; aucune erreur de console en `http` ; manifeste à `start_url` relative et service
worker (hors-ligne) ; aucun débordement horizontal au téléphone.

## Second volet : la conception par un modèle de design (28 octobre 2026)

Après #36, l'utilisateur : « c'est toujours moche, c'est pas comme Claude, regarde juste la
différence de niveau ». Défauts nommés : trop de cartes colorées, mise en page cassée, pas de
hiérarchie, typographie et couleurs. Ses décisions :

- **Priorité** : que Claw fasse beau seul ; l'appli de budget elle-même n'est pas retouchée.
- **Style** à donner si Claw le demande : **néobanque** (carte du solde en dégradé, pastilles
  rondes, l'esprit Revolut ou Lydia).
- **Modèles** : le meilleur gratuit en design pour la **conception** (« plan + base CSS ») ;
  ensuite **Nemotron 3 Ultra fait tout, jusqu'au bout** : plus de relais du tour à un autre
  modèle (les relecteurs peuvent toujours donner leur avis). Modèle choisi sans banc.
- **Livraison** comme avant : essai réel (2 passages au plus), comparaison avec #35 et #36,
  fusion directe dans `main`.

Choix du modèle de conception (classement *Design Arena*, sites web, octobre 2026) : Kimi K3
(Elo ~1334) n'est pas gratuit ; **GLM-5.3** (~1308) l'est chez NVIDIA, lent à l'essai d'outil
du catalogue (pas de réponse en 45 s le 28 octobre) mais la conception n'appelle aucun outil.
Inkling (~1224) était prévu en second, mais OpenRouter le refuse à la clé de Claw (« only
available on agentic harnesses », 403, déjà vu en session #32) : remplacé par **Gemini 3.6
Flash** (clé Gemini de Claw Web, répond en 2 s) puis **MiniMax M3.1 Flash** (NVIDIA, 3 s).
Réglage de Claw Web : `CLAW_DESIGNER_MODEL=nvidia:z-ai/glm-5.3,gemini:gemini-3.6-flash,nvidia:minimaxai/minimax-m3.1-flash`,
`CLAW_ESCALATION_MODEL=off`, `CLAW_DESIGN_MODEL=off`.

Premier essai (session #37, 28 octobre 2026, arrêté au bout de 3 min) : « aucun modèle de
conception n'a répondu (GLM-5.3 : pas de réponse après 20 s) ». Une consultation donnait 20 s
à chaque modèle pour un premier signe de vie, et Inkling avait été écarté par son refus. La
patience porte désormais ce délai (`Patience::first_sign`) : 2 min pour la conception, 20 s
ailleurs comme avant.

Deuxième essai (session #38) : GLM-5.3 muet même en 2 min, **Gemini 3.6 Flash** a conçu (plan
et `base.css` de 102 lignes, Newsreader et Plus Jakarta Sans). Mais Nemotron a réécrit
`base.css` de mémoire, sans l'avoir lu, dès sa deuxième écriture. Depuis : la note lui dit de
lire la feuille et de ne jamais l'écrire ni la modifier, et si elle change pendant le tour,
Claw la remet en place et lui dit de mettre ses règles dans `styles.css`
(`design_base_guard`, `design_base_restored`).

| Lot | Contenu | État |
|---|---|---|
| 10. Conception | Réglage `designerModel` (texte ou liste ; `CLAW_DESIGNER_MODEL`, `off` le coupe). Au début d'un tour de création d'interface (méthode de design jointe, pas de page déjà là sauf refonte demandée), le runtime demande au modèle de conception (`ApiClient::design_conception`, patience `Patience::DESIGN` : 2 min pour un premier signe de vie, 7 min par modèle, 12 en tout, 32 000 jetons) un **plan** (brief, direction, 5 à 8 couleurs nommées, deux polices de la bibliothèque de l'outil `Font`, espacements, écrans bloc par bloc, composants, mouvement, marques interdites dont « plusieurs cartes de couleurs différentes côte à côte ») et **`base.css`** (jetons que lit le kit : `--paper`, `--surface`, `--ink`, `--muted`, `--line`, `--accent`, `--on-accent`, `--income`, `--error`, `--field`, `--radius`, `--font-display`, `--font-text` ; clair et sombre ; le socle ; l'allure de base des titres, montants, boutons, surfaces, lignes de liste à pastille, champs, puces). Claw écrit `base.css` (marqué en tête ; `claw-base.css` si l'utilisateur a déjà son `base.css`), joint le plan à la demande (`conception_note` : ne pas écrire d'autre plan, lier `base.css` avant sa propre feuille, ne jamais redéfinir ses jetons), et la revue du plan est sautée (le plan est celui du concepteur). Sans réponse : le modèle de la session conçoit lui-même, comme avant (`web_design.rs` : `designer_prompt`, `parse_conception`, `has_interface_page` ; `conversation.rs` : `design_conception`) | fait (test de conversation, tests unitaires) |
| 11. Nemotron jusqu'au bout | Claw Web (`run.py`, `model_settings`) : conception par GLM-5.3, puis Gemini 3.6 Flash et MiniMax M3.1 Flash, ni escalade ni tour de design confié à un autre modèle ; le faux modèle n'appelle aucun modèle de conception | fait (test Python) |
| 12. Plancher néobanque | Kit `entries` : chaque opération en ligne de néobanque (pastille ronde de la catégorie, nom, « catégorie · jour » dessous, montant à droite, dernière ligne sans trait). Règle du composant : un montant donné en fourchette compte à son bout le moins favorable (le bas d'un gain, le haut d'une dépense), un montant marqué non sûr reste dehors jusqu'à sa réception, le meilleur des cas peut s'afficher à côté | fait (essais navigateur du kit) |
| 13. Livraison | Vérification complète, essai réel (2 passages au plus, « Néobanque » si Claw demande le style), comparaison avec #35 et #36, fusion directe dans `main` | repris au lot 17 (troisième volet) : #37 arrêté au bout de 3 min, #38 arrêté à 4 h 49 |

## Troisième volet : le kit d'appli refait (28 octobre 2026)

Le 28 octobre 2026 vers 4 h 40, l'utilisateur, sur l'appli de #36 et l'essai #38 en cours :
« Le site fait vieux et pas du tout à la hauteur de mes exigences, c'est juste moche et plein
d'incohérences et bugs graphiques ». Choix demandé (deux sessions travaillaient en même
temps) : **« Tout reprendre ici »** (session `017wjLwq`), l'autre session arrêtée ; #38
arrêté à 4 h 49. Ce volet part de la branche `claude/eloquent-mccarthy-17goou` (lots 10 à
12) et garde ses décisions (gratuit, conception par un modèle de design, Nemotron jusqu'au
bout, style néobanque).

Ce qui se voyait au téléphone (captures de #36 rejouées ; captures de #38 sur la branche
`claw-web-fichiers` de `claw-sessions`) : #36, pas de barre du haut, carte du solde collée
en haut, cartes pastel bordées (vert, brun, jaune), « Alimentaire-100,00 € » (libellé
collé au montant), « Dernières opérations » et « Voir tout » coupés sur deux lignes, dates
collées au bord de l'écran, noms des douze mois du bilan superposés, champs des réglages
sortis de leur carte, « 1235.15 » à côté de « 1 235,15 € ». #38 : Newsreader (empattements)
pour les titres et les montants, « Budget Personal », « + + Saisie rapide », grande carte
blanche vide pour le solde. L'appli de Claude (`budget_claude`) sert de mesure : carte du
solde en dégradé, cartes neutres, lignes à pastilles, barre d'onglets flottante avec bouton
d'ajout, volet d'ajout soigné, police du téléphone, clair et sombre.

Causes dans Claw : la méthode et le juge d'un site vitrine appliqués à une appli (police
« à soi », jamais `system-ui`, titre d'affiche, ornements), un kit fait de composants que le
modèle recopiait et retouchait, et des mesures aveugles à ces défauts (couleurs
`color-mix()` jamais lues, textes qui se touchent, étiquettes superposées, élément sorti de
sa carte, cadre de page en `overflow-x: hidden` qui cachait les débordements).

| Lot | Contenu | État |
|---|---|---|
| 14. Kit d'appli refait (`a2de1f1`) | Composant `app-kit` (les noms `app-tabs`, `entries`, `entry-sheet`, `chart` y mènent) : l'outil `Component` écrit `kit/app-kit.css` et `kit/app-kit.js` dans le dossier de la page (`dir`), rend le squelette de la page et un exemple de script d'appli (`app-kit-example.js`, l'appli de budget de la demande). Barre du haut avec le mois, écrans derrière une barre d'onglets flottante avec bouton d'ajout, carte du solde en dégradé (teinte de l'accent tournée), pavés, cartes et lignes à pastilles, pastilles d'état, jauges, graphique (noms des mois espacés), réglages, état vide, volet d'ajout et de modification (montant en grand, montants rapides, catégories), messages avec « Annuler », clair et sombre. Jetons posés sans poids (`:where`) : ceux de l'appli gagnent toujours ; texte sur l'accent et accent en texte lisibles quel que soit l'accent. Mesures : toute couleur CSS lue (pas seulement `rgb()`) ; une appli (barre d'onglets et écrans) n'est plus poussée vers un titre d'affiche ni une police « à soi » | fait (essai navigateur du kit) |
| 15. Méthode d'appli (`a02908e`) | `is_personal_app` (appli, app, PWA, avec budget, perso, carnet, suivi…) : méthode d'appli (`app_method` : les chiffres de l'utilisateur d'abord, le kit en un appel, le squelette suivi, les blocs du kit seulement, la police du téléphone, le parcours vérifié en un appel), concepteur d'appli (jetons de couleur et plan des écrans en blocs du kit, aucune police), note de conception (lier `kit/app-kit.css`, puis `base.css`, puis la feuille de l'appli), juge et revue du plan d'une appli de téléphone (Revolut, Lydia), pas d'un site. Kit : accent en texte mêlé à l'encre (juste aussi en sombre imposé), graphique de l'exemple dessiné dès le premier lancement | fait (tests de conversation et unitaires) |
| 16. Bugs graphiques mesurés (`df1e9cd`) | Audit de `Screenshot` : textes qui se touchent sur une ligne, étiquettes d'un graphique superposées, élément sorti sur le côté de sa carte, libellé court coupé sur deux lignes, nombres écrits de deux façons (page en français), texte collé au bord étendu aux dates et montants, cadre de page en `overflow-x: hidden` ignoré, contenu d'un `<details>` fermé non compté | fait (essai navigateur des défauts de #36, aucune fausse alerte sur l'appli de Claude ni sur le kit) |
| 17. Livraison | Essai réel #39 avec l'accord de l'utilisateur (deux passages au plus), comparaison avec #35, #36 et #38, fusion directe dans `main` | fait (deux passages, 11/12 au banc t9 ; fusionné dans `main`) |
| 18. Défauts de tous les écrans (`5dbda4b`, `369c4ab`, compte des onglets par position) | Les bugs graphiques mesurés aussi dans les écrans et les fenêtres ouverts (montants, textes coupés ou serrés, libellés sur deux lignes, débordements, icônes, emoji), avec les règles CSS en cause ; une fenêtre ouverte mesurée seule (la page couverte ne compte plus) ; choix d'un groupe (`label` à bouton radio) coupé sur deux lignes ; bouton « + » décentré dans la barre d'onglets, les onglets comptés de part et d'autre de lui (une cale vide ou deux blocs d'onglets, vus dans le second passage de #39, ne le centrent pas) ; barre fixée en bas posée au bout de la page dans les captures de la page entière (elle se dessinait au milieu, par-dessus les cartes) ; rang de puces qui défile d'un bord à l'autre accepté ; bouton « Fermer » d'un formulaire en `method="dialog"` laissé fermer sa fenêtre (il passait pour un bouton qui ne ferme pas). Kit : dépenses du graphique en `--error` (deux verts quand l'accent est vert), pas d'anneau sur le titre d'un écran qui reçoit le focus, choix du volet avec une marge | fait (essais navigateur, aucune fausse alerte sur l'appli de Claude ni sur le kit) |
| 19. Feuille rapiécée signalée (`4829e33`) | Dès l'écriture d'un fichier CSS, parmi les pièges du code : la même propriété d'un même élément posée dans trois règles ou plus au même endroit, ou dans cinq règles de plus en plus longues (`.seg span`, `.sheet .seg span`, `dialog.sheet .seg span`), et plus de vingt `!important` ; le modèle est invité à garder une seule règle et à la changer sur place. Sur 23 feuilles (les applis de Claw et de Claude, le kit, ClaudeRecap), seule celle de #39 est relevée | fait (tests unitaires) ; après le second passage, qui part du lot 18 |

Essai réel #39 (moteur `6dd8bcf`, Nemotron 3 Ultra, premier passage de 6 h 52 à 8 h 42,
238 requêtes) : appli de style néobanque en vert, carte du solde en dégradé, barre d'onglets
flottante, volet d'ajout, clair et sombre ; les montants de l'utilisateur au premier lancement,
« 589,60 € » et « 103 à 206 € » justes à la fin (« 589,6 € » et « 206 € € » en cours de
route). Banc t9 : 11/12 (#35 : 4/12, #36 : 8/12 ; #38 arrêté), seul « sombre même sur un
téléphone en clair » manque, comme le kit seul. Ce qui reste : le choix coché du volet
d'ajout invisible en clair (1,0:1 de contraste, blanc sur blanc), que l'agent n'a pas su
réparer en quinze règles empilées sur le même élément (`styles.css` de 1 091 lignes, 217
`!important`) ; le bouton « + » entre le deuxième et le troisième onglet (un onglet retiré) ;
« charges incompressibles » serré mot par mot dans l'écran Analyse ; dans les captures, les
titres d'écran encadrés (anneau du focus du kit) et la barre d'onglets au milieu de la page,
que le relecteur avait fait « fixer » alors qu'elle l'était (elle a perdu sa forme flottante).
Le lot 18 mesure ces défauts et nomme les règles en cause ; le second passage (« continue »)
part de ce moteur.

Second passage (moteur `5dbda4b`, de 10 h 14 à 12 h 04, 309 requêtes) : les trois défauts
mesurés au départ réparés (choix coché lisible en clair et en sombre, texte serré, bouton
« + » dit centré), puis l'accueil refait sur les remarques de la revue visuelle (carte
« Résumé du mois » retirée, carte du solde épurée, « Opérations récentes »), les titres
d'écran sans cadre. Mesures du moteur sur l'appli finale (page, fenêtre et écrans) : aucun
défaut ; banc t9 : 11/12 (le même manque). Ce que l'oeil voit encore : le bouton « + »
centré par deux blocs d'onglets (deux à gauche, un à droite, serrés), mesuré depuis
(`369c4ab` et le compte par position) ; la carte du graphique d'Analyse un peu décalée ;
`styles.css` de 1 416 lignes et 339 `!important`, que le lot 19 signale désormais dès
l'écriture. À l'ordinateur, l'appli s'affiche sans erreur (la capture blanche de fin de
passage était passagère).

Banc t9-budget sur l'appli d'exemple du kit (squelette et exemple tels quels) : 11/12 quand
elle suit le téléphone (seul « sombre même sur un téléphone en clair » manque, la demande
n'ayant pas « sobre foncé »), 12/12 avec des jetons sombres imposés comme le concepteur les
écrirait. Mesures de `Screenshot` (phone et desktop, clair et sombre, fenêtres et écrans
explorés) : aucun défaut.

Essais : `the_app_kit_makes_a_budget_app_that_works_in_a_real_browser` (l'appli du kit,
mesurée et parcourue : ajout, modification, suppression, « Annuler » ; `CLAW_KIT_DEMO=dossier`
en garde une copie), `the_app_kit_is_written_into_the_page_folder_and_the_old_names_lead_to_it`,
`the_graphic_bugs_of_session_36_are_measured_in_a_real_browser`,
`the_graphic_bugs_of_session_39_are_measured_in_every_screen_in_a_real_browser`,
`a_personal_app_gets_the_app_method_the_app_designer_and_the_app_judge`.

## Quatrième volet : CSS propre, juge recalé, vitesse, essais (28 octobre 2026)

« Qu'est-ce que tu peux faire pour Claw ? » : l'utilisateur a choisi les quatre chantiers
proposés (CSS propre, revue visuelle juste, plus rapide, essais sur d'autres applis), avec
son accord pour la jauge du juge et un essai réel « outil perso ».

Ce que disent les journaux de #39 (second passage : 105 min, 263 requêtes) : 24 captures pour
32 min ; 56 lectures de fichiers, dont 33 de `styles.css` par morceaux de 150 lignes et 9 du
kit ; 68 recherches, la plupart ne rendant que « styles.css » ; quatre revues visuelles de
5,0 à 5,7 sur 10, mêmes huit remarques, parfois contraires (« empiler les pavés », puis « les
mettre côte à côte »), des survols demandés pour une appli de téléphone. Les consignes du
juge disaient « most generated apps deserve 4 to 6 » : avec la barre à 8, la boucle ne
pouvait finir qu'au bout du temps. Le kit lui-même a été modifié au second passage (une
règle de largeur ajoutée dans `kit/app-kit.css`).

| Lot | Contenu | État |
|---|---|---|
| 20. Recherche qui montre les lignes | `grep_search` sans mode : les lignes trouvées quand la recherche vise un fichier ou trouve peu (5 fichiers et 40 lignes au plus, 120 lignes rendues) ; sinon la liste des fichiers, comme avant | fait (tests unitaires) |
| 21. CSS propre | Méthode d'appli : pas de `!important` (les règles du kit sont légères), une règle changée là où elle est ; kit écrit avec son empreinte en tête, remis à la version de Claw au début d'une session quand la copie est intacte (et les copies d'avant les empreintes, connues par leur empreinte) ; écrire dans le kit est un piège signalé dès l'écriture | fait (tests unitaires) |
| 22. Juge recalé et jauge | Juge d'une appli : une appli propre qui montre les chiffres de l'utilisateur vaut 8, sur le kit elle part de là, et chaque point retiré se montre sur une capture ; quatre changements au plus ; jamais de survol ; les changements de la revue d'avant lui sont donnés, sans leur contraire. Jauge : `claw design-replay PAGE [DEMANDE]` note une page comme une revue du tour, sans agent ; la session réservée `jauge-<n>` du workflow note `jauge/*/index.html` d'une branche (kit seul, appli de Claude, appli de Claw #39 dans `budget_claw`) et range le résultat sur `claw-web-fichiers` (`jauge/<n>.json`) | fait ; jauge à lancer |
| 23. Goûts gardés | Mémoire du projet (appli web) : les phrases d'un message qui disent un goût de style (sombre, clair, sobre, néobanque, couleurs, police…) gardées, six au plus, en tête de `.claw/memory.md` ; le concepteur, la revue du plan, le juge visuel et le thème des captures les reçoivent avec la demande | fait (tests unitaires) |
| 24. Barre calée sur le kit | Jauge-1 (13 h 03, sur claw-pc, juge Nemotron 3.5 VL) : le kit seul 6,9/10, l'appli de Claw #39 7,7, celle de Claude 5,3 (13 défauts selon nos règles : emoji, textes de 11 px, contrastes). Choix de l'utilisateur : la barre d'une appli perso est la note du kit seul avec le juge du moment, tirée par le moteur de la dernière jauge (`CLAW_APP_DESIGN_BAR`, 7 sans jauge, jamais au-dessus de celle d'un site) ; celle d'un site reste 8 | fait (tests unitaires) |
| 25. Essai réel « outil perso » | Un suivi d'habitudes au téléphone (nouvelle session, deux passages au plus), comparé au budget | à faire |

## Suite, dans l'ordre

Le chantier est fusionné dans `main`. Ce que l'essai réel a laissé (session #36, ci-dessous) :

1. **Le kit recopié au lieu d'être collé** : l'agent a gardé l'architecture du kit
   (`window.entries`, `drawChart`, `screenchange`) mais a tout retapé dans un `app.js` de 829
   lignes, et y a mis deux bogues que le kit n'a pas (graphique `NaN`, liste du mois vide),
   réparés ensuite. Piste : que l'outil `Component` écrive les scripts du kit dans le projet
   (`components/entries.js`…), à inclure par `<script src>`, au lieu de les rendre en texte.
2. **La CAF de 103 à 206 €** a été prise pour un gain non assuré : le reste prudent affiché
   (745,55 €) l'oublie. Dit depuis le lot 12 dans la règle du composant `entries` (le bas
   d'une fourchette de gain compte) ; à vérifier sur t9.
3. **Pas de question posée** dans #36 : le style (« sobre foncé » dans #35) n'a pas été
   demandé, et l'appli suit le réglage du téléphone, claire par défaut. Depuis le lot 10, la
   direction vient du modèle de conception.
4. Banc t9 avec le banc d'essai (`run.py t9-budget`), et un second passage de #36
   (« continue ») si l'utilisateur le veut : il reste 9 tâches à l'agent.

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

Fait le 27 octobre 2026 : session #36 de `claw-sessions` (même demande que #35, même modèle
`nvidia:nvidia/nemotron-3-ultra-550b-a55b`, exécuteur `claw-pc`, `budget_claw` branche
`claw/session-10`), moteur de la branche (`f08985c`) par la branche
`claude/eloquent-mccarthy-17goou` de `claw-sessions`. Un seul passage : Claw n'a posé aucune
question et a construit directement ; le second passage n'a pas été lancé, le quota gratuit
du jour (relecteurs, lecteur d'images) étant épuisé (1004/1000).

| | #35 (moteur d'avant) | #36 (ce chantier) |
|---|---|---|
| Durée du passage de travail | 109 min, arrêté par l'heure | 62 min 31 s, arrêté par la limite de 300 requêtes |
| Requêtes | 304, moyenne 21,5 s (médiane 9 s) | 301, moyenne 11,6 s (médiane 7 s) à mi-parcours |
| Composants | aucun du kit | `app-tabs`, `entries`, `entry-sheet`, `chart`, `pwa`, `toast`, `dialog` ; icônes PNG par `AppIcon` |
| Banc t9 (`check.py`) | 4/12 | 8/12 |
| Montants au premier lancement | absents (0,00 € partout) | là (sauf le haut de la CAF, 206) |
| Graphique | vide (canvas 0x0) | dessiné (SVG, accueil et bilan) |
| Hors-ligne et installable | `start_url` `/`, pas de service worker, icône SVG seule | manifeste à `start_url` `./`, service worker, PNG 192, 512, maskable, 180 |
| Texte tapé lisible | noir sur noir en sombre | noir sur noir en sombre dans l'écran « Réglages » (mesure ajoutée depuis) |
| Reste prudent / au mieux | absents | 745,55 € (la CAF comptée comme incertaine), pas de meilleur des cas |
| Sombre | clair imposé malgré « sobre foncé » | suit le téléphone (aucune question posée) |

Corrections tirées de #36, faites sur la branche avant la fusion : parcours `Browser` écrits
autrement (`actions`, `steps` sans `action`, `into`, étapes de Screenshot) mis à la forme ;
une entrée ajoutée absente de sa liste n'est plus dite « nom affiché nulle part » (l'agent
ajoutait un champ « Nom » à la dépense) ; les champs d'un écran caché ou d'une fenêtre sont
mesurés aussi dans l'autre mode quand la page suit le réglage du téléphone.

Pour essayer une branche sans toucher `main` :

1. Dans `abasse-ali/claw-sessions`, sur une branche, faire pointer le workflow
   `.github/workflows/claw-web.yml` sur `abasse-ali/claw_code/.github/workflows/claw-web.yml@<branche>`
   et lui passer `claw_ref: <branche>`.
2. Créer la session comme l'appli : une issue étiquetée `claw-web`, le prompt du budget, et
   en fin de corps `<!-- claw-web {"repo":"abasse-ali/budget_claw","base":"main","model":"nvidia:nvidia/nemotron-3-ultra-550b-a55b","n":<n>,"branch":"claw/session-<n>"} -->`.
3. Lancer le workflow sur cette branche (`repo`, `base`, `session` = numéro de l'issue,
   `model`, `runner` = `claw-pc` comme #35). Répondre aux questions comme #35 (« Fonctions :
   Tout (Recommandé) », « Style : Sobre foncé »), puis relancer.
4. Comparer avec #35 : durée, requêtes, vérifications cachées ci-dessus.
