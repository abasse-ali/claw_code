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
