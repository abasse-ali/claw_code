# C2. Applis en React et bibliothèques prêtes ; Claw seul avec son modèle

Point d'entrée de toute session qui reprend ce chantier : ce qui a été décidé, ce qui est fait,
ce qui reste, et comment vérifier.

## La demande (28 octobre 2026)

> Claw qui fait tourner un model ne demande plus aux autres models quoi que ce soit faut les
> descriptions d'image. Et dans mettre en ligne ajoute un bouton pour « dépublier le site ».
> Peut-être pour aider Claw ce serait avoir un maximum de bibliothèque, comme par exemple les
> bibliothèques de Widget des codes pré configurés, Vite, NodeJS, React. Développé avec React
> TypeScript Tailwind CSS Vite Framer Motion Node.js Express Python MySQL Redis Socket.IO Rust
> WebAssembly.

Réponses de l'utilisateur aux questions :

- Autres modèles : **garder la conception et le juge** (et les descriptions d'image). Coupés :
  la relecture finale, l'avis quand le modèle bloque, la relecture du plan, l'outil `Consult`.
- Bibliothèques : « **je veux qu'il puisse directement les utiliser au lieu de coder à la
  main** ».
- Par défaut : **React + Vite** pour les applis.

## Ce qui est fait

1. **Autres modèles** (`claw-cli/src/models.rs`, `runtime/src/conversation.rs`, `run.py`) :
   `CLAW_ADVISOR_MODELS=off` (aussi `none`, `non`, `aucun`, `false`, `0`) vide la liste des
   conseillers ; `ApiClient::can_consult()` est faux sans conseiller, et aucun avis n'est demandé
   ni annoncé. Claw Web pose `off` par défaut. Le concepteur (`CLAW_DESIGNER_MODEL`), le juge
   visuel et le lecteur d'images restent.
2. **« Dépublier le site »** (`claw-web/js/app.js`, `github.js`) : dans « Mettre en ligne », un
   site en ligne a son bouton (deux appuis), `DELETE /repos/{dépôt}/pages`.
3. **Projets de départ, outil `Starter`** (`tools/src/starters.rs`, modèles dans
   `tools/src/starters/`) :
   - `react-app` (par défaut) : React 19, TypeScript 7, Vite 8, Tailwind CSS 4, Framer Motion,
     lucide-react, zustand, recharts, radix-ui, date-fns, zod, canvas-confetti, clsx, aux
     versions figées. Le kit d'appli de Claw en composants React dans `src/kit/` (AppShell,
     Hero, Tiles, Card, List et Row animées, Meter, BarChart, MonthsChart, Settings, MoneyInput,
     Switch, EntrySheet, Sheet, Modal, ConfirmModal, toast avec « Annuler », données gardées
     sur le téléphone avec zustand, copie CSV), habillé par le même `app-kit.css` que les
     applis sans React (couche `components` de Tailwind : une classe de Tailwind l'emporte sur
     le kit). `src/App.tsx` est un exemple qui marche (un budget du mois), sans donnée
     inventée. Construite dans `docs/` (adresses relatives), la page que GitHub Pages publie ;
     hors ligne par `public/sw.js`.
   - `node-api` : Express 5, Socket.IO, cors, TypeScript lancé par tsx, tests `node:test`.
   - `python-api` : FastAPI, WebSocket, pydantic, tests pytest, bibliothèques dans `.venv`.
   - Les fichiers de `src/kit/` sont récrits à chaque appel ; les autres ne sont écrits que
     s'ils manquent. L'outil installe les bibliothèques (`npm install`, ou `pip` dans `.venv`).
   - `src/index.css` importe la feuille du concepteur (`base.css`, ou `claw-base.css`) avant
     `src/theme.css` quand elle est là.
4. **Construire avant de mesurer** (`tools/src/vite.rs`, `screenshot::page_url`) : la page
   d'une appli Vite (son `index.html` qui charge `/src/…`) est construite avant chaque
   `Screenshot`, `Browser` et `ViewPage` : bibliothèques installées s'il le faut, `npm run
   build` quand une source est plus récente que la construction, puis la page construite
   s'ouvre. Une erreur de construction revient au modèle avec sa cause, sans couleurs de
   terminal ni pile d'appels.
5. **Vérification** : `npm run typecheck` (choisi d'office par Claw après les modifications)
   installe ce qui manque (`pretypecheck`), vérifie les types (`tsc --noEmit`), puis construit
   (`posttypecheck`) : `docs/` est à jour à la fin du passage.
6. **Méthode** (`runtime/src/web_design.rs`) : une appli perso se construit en React sur
   `Starter` (`react_app_method`) ; le kit sans React reste pour une demande de « HTML simple »
   (« sans framework », « vanilla »…) ou une appli déjà faite sans React dans le projet
   (`kit/app-kit.js`). La note du concepteur et le rappel de `base.css` suivent le même choix.
7. **Journal de Claw Web** : « Prépare le projet · appli React · 27 fichiers ».

## Essai réel #41 (28 octobre 2026, `budget_claw`, branche `claw/session-15`)

La demande de la session #35, sur `main` après la fusion (#127), sur l'exécuteur `claw-cloud`
(ARM64). Claw appelle `Starter` au premier tour ; installation, `npm run typecheck` et
construction marchent ; deux erreurs de TypeScript trouvées et corrigées par le modèle. Mais
ses captures lui signalaient à tort, à cause du kit React :

- des liens vers des écrans « qui n'existent pas » (`#analyse`, `#reglages`) : seul l'écran
  ouvert était dans la page. Du même coup la revue ne reconnaissait pas une appli à onglets et
  réclamait un grand titre. Corrigé : `AppShell` garde chaque écran dans la page, les autres
  cachés (`hidden`), comme le kit sans React ;
- des icônes « dessinées à la main » : `settings`, `receipt` et `shopping-cart` de lucide-react
  1.63 ont un tracé plus récent que celui de Claw. Corrigé : une icône marquée
  `class="lucide lucide-<nom>"` par un paquet de Lucide vient du jeu (`icon_set::exempt`) ;
- trois pastilles de la carte du solde serrées (« Revenus prudents » sur 47 px), et des
  lettres trop rapprochées sous 32 px. Corrigé dans `app-kit.css` (pour les deux kits) : deux
  pastilles par ligne au téléphone, `letter-spacing: -0.01em` pour les tuiles et les titres.

L'essai de bout en bout vérifie désormais ces défauts aussi, avec trois pastilles.

Bilan du passage (47 min de travail, 124 actions, une seule session) : les chiffres de
l'utilisateur dès l'ouverture, reste prudent 848,55 €, cas favorable 1 151,55 €, ajout,
modification, suppression avec « Annuler », onglets, réglages ; note du juge bloquée vers
6/10 (barre 8), en partie à cause des fausses alertes ci-dessus. Banc t9 : **10/12** (#35 :
4/12, #36 : 8/12, #39 : 11/12). Manqués : pas d'icône PNG (le modèle n'a pas appelé
`AppIcon`) ; sombre non imposé (la réponse « sobre foncé » n'est pas dans la demande de
l'essai). Le banc lui-même avait un défaut : il ne lisait que le dernier écran visité après
le rechargement (la dépense, gardée, était sous l'onglet Historique) ; il lit maintenant les
montants de chaque écran, et sert `docs/` pour une appli Vite.

Corrigé ensuite :

- **Icônes PNG automatiques** (`vite::app_icons`) : à chaque construction, quand
  `public/icon.svg` est plus récent qu'elles, Claw dessine les icônes PNG (192, 512,
  maskable, iPhone) avec l'outil `AppIcon`, les ajoute au manifeste et lie celle de l'iPhone
  dans `index.html`.
- **« La page s'ouvre avec un code »** à tort : le détecteur de codes d'accès lisait
  `package-lock.json` (`"sourcemap-codec": "^1.5.0"`, `trace-mapping`). Une version n'est plus
  un code, et les textes de la page ne comprennent plus `package-lock.json` ni la construction
  d'une appli Vite (`docs/` à côté d'un `vite.config`).

### Où sont passées les 61 minutes de #41

Le modèle a travaillé 47 minutes (116 requêtes, 121 outils), puis la session a mis 14 minutes
de plus à finir :

| Poste | Temps | Part |
|---|---|---|
| Captures demandées par le modèle (13, souvent après une ligne de CSS ; chacune décrite par un modèle qui voit, Nemotron ne lisant pas les images) | 12 min 29 s | 27 % |
| Revues visuelles du juge (5 : 5,4 puis 6,0, 6,3, 6,0, 5,9/10) | 10 min 37 s | 23 % |
| Écritures et modifications (40, dont 3 ratées) | 7 min 16 s | 16 % |
| Vérification des types et construction (33 commandes) | 6 min 04 s | 13 % |
| Réflexion, réponse, compactage du contexte | 3 min 55 s | 8 % |
| Lectures (21, surtout le kit au début) | 3 min 16 s | 7 % |
| Parcours dans le navigateur (9) | 3 min 05 s | 7 % |
| **Après la fin** : traduction en français des remarques des captures, 11 échecs de 70 à 90 s mis en file, que le programme attendait avant de pousser la branche | 13 min | |

Les défauts visibles viennent de deux causes :

1. Les fausses alertes de Claw (liens, icônes, titre, pastilles, faux code) : une dizaine
   d'allers-retours sur des défauts qui n'en étaient pas (corrigé ci-dessus).
2. Le modèle a récrit l'allure du kit dans `src/theme.css` (33 règles sur ses blocs) au lieu
   d'y poser des couleurs : c'est là que naissent la légende illisible et les pastilles collées
   à leur bord. Rien ne le lui disait.

Corrigé :

- **Traduction** (`.github/claw-web/translate.py`) : un fil d'arrière-plan qui ne retient plus la
  fin du programme, une attente finale bornée à 30 s en tout, plus aucun appel après deux échecs
  de suite, délai de 45 s au lieu de 90.
- **Kit récrit** (`code_checks::kit_restyled`) : la feuille de l'appli posée à côté de
  `kit/app-kit.css` (React : `src/theme.css` ; sans React : `styles.css`) qui vise les classes
  du kit est signalée dès l'écriture, avec ses règles ; des jetons posés pour un bloc
  (`.hero { --hero-1: … }`) restent permis. Sur le `theme.css` de #41 : 33 règles relevées à la
  ligne 51, dès la première écriture.

Décidé avec l'utilisateur le 29 octobre 2026 (« Oui les deux ») :

- **Revues en plateau** (`web_design::plateau`, `review_plateau`) : un progrès est une hausse
  d'un demi-point au moins de la meilleure note d'avant (`MIN_GAIN`, 0,2 auparavant ; en
  dessous, c'est l'humeur du juge). Après deux revues de suite sans progrès, Claw arrête les
  revues du tour (ni revue de base, ni poursuite, ni revue « de structure ») et le dit une fois,
  sauf si le fonctionnement est en train de se réparer. Sur #41 : 6,3 n'aurait pas valu de
  poursuite simple (+0,3), une revue de structure au plus, puis l'arrêt : 4 revues au lieu de 5.
- **Captures retenues** (`capture_check`) : le modèle redemande la même capture (même page,
  thème, tailles, actions) après une seule modification de l'interface alors que la précédente
  relevait 3 problèmes ou plus : Claw lui rend la liste de ces problèmes à corriger d'abord, sans
  lancer le navigateur. Redemandée tout de suite, la capture passe (une seule retenue par capture).
  Sans aucune modification, la même capture rendait déjà son résultat sans nouveau rendu.

## Non fait, et pourquoi

- **MySQL et Redis** : ni sur le PC de Claw ni sur GitHub Pages ; les serveurs de départ gardent
  leurs données dans un fichier JSON. À ajouter quand un hébergeur avec base de données sera
  choisi.
- **Rust et WebAssembly** : la cible `wasm32-unknown-unknown` et `wasm-bindgen` ne sont pas sur
  le PC ; un projet de départ sans pouvoir le construire ni le tester serait un piège.
- **Serveurs en ligne** : GitHub Pages ne publie que des pages statiques ; `node-api` et
  `python-api` demandent un hébergeur (Render, Railway, Fly.io, un VPS), ce que leur mode
  d'emploi dit au modèle.

## Vérifier

- `cargo test -p tools --lib -- starters vite` ; l'essai de bout en bout, qui installe les
  bibliothèques et lance le navigateur :
  `CLAW_BROWSER=<chrome> cargo test -p tools --lib -- --ignored the_react_starter_builds_and_works_in_a_real_browser`
  (dans un conteneur en root, un lanceur qui ajoute `--no-sandbox`). Le 28 octobre 2026 : zéro
  défaut mesuré par la revue de Claw (téléphone et ordinateur, clair et sombre, fenêtres
  ouvertes), parcours ajout, modification, suppression, « Annuler » et onglets réussi,
  reconstruction après modification, erreur de construction rendue avec sa cause.
- `cargo test -p runtime --lib -- web_design conception` pour la méthode.
- `node-api` : `npm test` (3 tests) ; `python-api` : `.venv/bin/python -m pytest` (3 tests).

## Kit : l'argent du mois, et les widgets revus (31 octobre 2026)

Demande : « Lis les kits de Claw code et regarde la session en cours, il me faut plus de widgets
dans le kit et corrige ceux qui sont déjà en place. » Session en cours : #46 (`claw-projet`,
appli de budget à partir des relevés de la banque), où le modèle avait écrit lui-même, dans
`src/kit/`, les dépenses et rentrées de chaque mois (qui se rajoutaient à chaque changement de
mois, « Tu mens c'est toujours présent »), l'import du relevé, le classement et un calendrier.

- **Nouveau `kit/money.tsx`** :
  - `RecurringList` et `useRecurring` : chaque rentrée ou dépense de chaque mois devient une vraie
    opération une fois par mois (identifiant `rec:<id>:<mois>`), jamais dans les mois d'avant sa
    création ; une opération supprimée ne revient pas.
  - `BankImport`, `importBankCsv`, `parseBankCsv` : relevé `.csv` (points-virgules ou virgules,
    JJ/MM/AAAA, « 1 234,56 », colonnes Débit/Crédit) ; une même opération de deux relevés qui se
    chevauchent n'est comptée qu'une fois, deux opérations pareilles le même jour restent deux.
  - `categorize` (CARREFOUR → Courses, SNCF → Transport…) et `recategorize`, qui retient le choix
    de l'utilisateur pour les libellés pareils.
  - `MoneyCalendar` : les jours de rentrée et de dépense du mois, la dépense du jour en petit,
    un jour touché montre ses opérations.
- **Widgets corrigés** (galerie de tous les widgets, mesurée au téléphone en clair et en sombre) :
  messages qui débordaient de leur carte, points du carrousel (8 px à toucher), croix des
  étiquettes, titres triables du tableau, action d'une alerte.
- Vérifié : 3 allers-retours entre les mois et 3 rechargements donnent une seule opération par
  mois ; deux relevés qui se chevauchent, rien en double ; essai de bout en bout du projet React
  dans un vrai navigateur.

## Kit : effets, nouveaux widgets et serveurs branchés (31 octobre 2026)

Demande : « Continue à trouver tous les widgets et animations pour le kit et ajoute tous ça au
kit : React TypeScript Tailwind CSS Vite Framer Motion Node.js Express Python MySQL Redis
Socket.IO Rust WebAssembly ».

- `kit/fx.tsx` (Framer Motion) : Typewriter, Marquee, Tilt, RippleButton, ShineText,
  FlipNumber, CountUp, Parallax, Float, Flash, SwipeDeck, SlideView, ScrollProgress. Tous
  s'arrêtent quand le téléphone demande moins de mouvement.
- `kit/plus.tsx` : Drawer, Popover (classe `bubble`, `popover` étant celle des menus radix),
  NumberPad, TimePicker, Kanban, Agenda, Gauge, GoalCard, PullToRefresh, LoadMore,
  Pagination, Breadcrumbs, Lightbox, SignaturePad, OfflineBanner/useOnline, SafeArea,
  IconBadge ; et les branchements useApi/sendApi (Express, FastAPI), useLive (Socket.IO,
  `socket.io-client` ajouté à l'appli), useWasm.
- `node-api` : les données dans un fichier JSON, MySQL (`DATABASE_URL`, mysql2) ou Redis
  (`REDIS_URL`, ioredis) ; les méthodes du magasin rendent des promesses ; une base injoignable
  donne une erreur 503 claire, ou un arrêt clair au démarrage.
- `python-api` : JSON, base SQL par SQLAlchemy (MySQL par PyMySQL, ou SQLite), ou Redis.
- Nouveau projet de départ `rust-wasm` : l'appli React plus `wasm/` (crate Rust sans
  wasm-bindgen, fonctions `extern "C"` sur des nombres), compilée en `public/calc.wasm` par
  `scripts/wasm.mjs` avant chaque construction (sans dossier `wasm/`, ce script ne fait rien).

Vérifié : galerie au téléphone (390 et 360 px, clair et sombre), sans débordement de page ;
pavé numérique, Kanban, panneau, visionneuse, bandeau hors ligne, SafeArea et module Rust
essayés dans Chromium. Serveur Node : 5 tests avec un vrai Redis et un vrai MariaDB (MySQL),
données gardées après redémarrage. Serveur Python : 6 tests (SQLite, Redis, MariaDB). GitHub
Pages ne publie que des pages : une appli publiée y garde ses données sur le téléphone, les
serveurs demandent un hébergeur (Render, Railway, Fly.io…).

### Suite : gestes et vues (31 octobre 2026, « Continue »)

- `kit/touch.tsx` : FlipCard (recto/verso), HoldButton (maintenir pour un geste sans retour),
  SpeedDial (le + à plusieurs actions), LikeButton (cœur qui éclate), Onboarding (écrans du
  premier lancement, une seule fois), Tour (visite guidée qui éclaire un élément), Stories,
  useLongPress.
- `kit/views.tsx` : WeekStrip, RangeSlider, DateRange et inPeriod, StackedBar, CompareBars
  (prévu face au réel), Poll, Leaderboard, AvatarGroup, Highlight (recherche sans accents ni
  majuscules), Wizard (formulaire en étapes, avec vérification par étape).
- Corrigé : en clair, les initiales des avatars étaient trop pâles (clarté 70 % sur fond
  clair) ; le texte posé sur l'accent prend `--on-accent`, l'accent étant clair en sombre.

Vérifié dans Chromium, clair et sombre, 360 et 390 px : chaque geste essayé (maintenir trop
court n'efface rien, l'accueil ne revient pas, le sondage compte juste, l'étape vide est
refusée).

### Suite : plus d'animations (31 octobre 2026, « Ajoute plus d'animation »)

`kit/anim.tsx` : TextReveal (mots un à un, sortant d'un flou), ScrambleText (lettres
mélangées qui se posent), Aurora (fond aux couleurs qui bougent), GlowCard (lumière qui suit
le doigt), Magnetic, Wiggle, ExpandCard (carte agrandie en plein écran, layoutId), AnimatedList
(blocs faits à la main ; une List de Row le fait déjà, et un Row dedans ferait un li dans un
li), LoadingBar, Twinkle, Breathe (cercle de respiration), TypingDots, LiquidFill (verre qui se
remplit en vagues), DrawPath, Particles (neige, cœurs, feuilles, confettis), Bump. Noms choisis
pour ne pas croiser une icône de lucide-react (Sparkles, Spotlight en sont).

Vérifié dans Chromium, clair, sombre et « moins de mouvement » (rien ne tombe, le code
s'affiche d'un coup, la barre de chargement est fixe) : chaque animation essayée, sans erreur.

### Suite : des blocs pour tous les genres d'applis (31 octobre 2026)

Demande : « continue à en trouver des widgets et animations pour le kit soit complet pour tout ».

- `kit/social.tsx` (boutique et réseau) : PriceTag, ProductCard, CartSummary (livraison offerte
  au-delà d'un montant), PostCard, CommentList, ProfileHeader, FollowButton, NotificationList.
- `kit/life.tsx` : AudioPlayer, VideoPlayer, ImageCompare ; ActivityRings, MoodPicker/moodOf ;
  Quiz, SpinWheel (s'arrête sous le repère sur la part annoncée) ; DaySchedule, AnalogClock
  (fuseaux) ; StackView/useStack/BackBar (écrans empilés, le retour du téléphone revient d'un
  écran sans quitter l'onglet), SlideToConfirm, ScrollTopButton, ThemeToggle (data-theme,
  retenu), PhoneField.
- Corrigé en essayant : durée d'un son inconnue (« Infinity:NaN » → « –:–– ») ; bouton Envoyer
  des commentaires qui sortait de sa carte.

Vérifié dans Chromium (clair et sombre, 390 px) : chaque bloc essayé, sans erreur.

### Suite : cartes, codes QR et notes (31 octobre 2026)

- `kit/map.tsx` : MapView (Leaflet, fonds OpenStreetMap ; épingles, trajet, onPick, onMarker),
  useGeolocation, LocateButton, PlaceSearch/searchPlace (Nominatim), distanceKm,
  formatDistance. En sombre, les fonds sont inversés ; hors ligne, la carte le dit.
- `kit/qr.tsx` : QRCode (SVG noir sur blanc, « Enregistrer l'image »), QRScanner (appareil
  photo, ou une photo ; jsQR).
- `kit/notes.tsx` : NoteEditor (Tiptap : titres, gras, italique, barré, listes, liste à cocher,
  citation, annuler), useNotes (gardées sur le téléphone), NotesList (recherche sans accents,
  épinglées en haut, suppression avec Annuler), noteText, noteTitle.
- Leaflet et Tiptap sont dans `src/kit/lazy/`, chargés au premier affichage (`React.lazy`) ;
  qrcode et jsQR par `import()` au premier usage. L'appli sans carte ni notes garde sa taille
  (581 Ko contre 578 Ko) ; la carte (150 Ko) et l'éditeur (404 Ko) viennent à part.
- Les styles de Leaflet, chargés hors des couches du kit, l'emportaient sur lui (boutons de
  zoom de 30 px, bulles blanches en sombre) : ses réglages changés sont en !important.

Vérifié dans Chromium (clair, sombre, 360 et 390 px) : fonds de carte et recherche d'adresse
simulés (OpenStreetMap est bloqué depuis l'environnement de test) ; épingles, trajet, bulle,
toucher qui pose un lieu, recherche, distance (tour Eiffel – Louvre : 3,2 km) ; code QR
enregistré puis relu depuis la photo ; lecture par une fausse caméra ; note avec liste à cocher
et gras, gardée après rechargement, cherchée, supprimée puis rétablie.
