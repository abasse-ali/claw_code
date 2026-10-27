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
| 5. Parcours en un appel | Browser accepte une liste d'étapes avec résultats attendus (`steps`), jouée d'un coup, au lieu d'un geste par requête (24 appels `Browser` dans #35) | à faire |
| 6. Kit d'appli | Composants testés pour les applis perso (stockage local avec annuler, export CSV et import ; volet de formulaire aux champs lisibles ; barre d'onglets ; graphique qui se redessine quand son écran s'ouvre ; PWA hors-ligne : manifeste à `start_url` relative, service worker, icônes PNG) ; le guide de design les propose pour une appli de données perso | à faire |
| 7. Vitesse | Après les lots 1 à 4 (moins de faux défauts, moins de messages redits) : mesurer sur le passage réel ce qui fait encore attendre (revues, juge, lecteur d'images, traductions des notes) et le couper ou le paralléliser, barres gardées à 8 | à faire |
| 8. Banc t9-budget | Scénario `rust/scripts/real-tests/scenarios/` avec ce prompt et les vérifications cachées ci-dessous | à faire |
| 9. Livraison | Vérification complète, essai réel (2 passages au plus), comparaison avec #35, **fusion directe dans `main`** | à faire |

Vérifications cachées prévues pour t9 (tirées de la comparaison) : montants présents au
premier lancement (`1235.15`, `103`, `206`, `80`, `21.6`) ; espèces comptées seulement une
fois reçues ; fourchette de la CAF (prudent et au mieux) ; mode sombre respecté quand il est
demandé ; texte tapé lisible ; graphique dessiné ; historique non inventé (mois d'avant
vides) ; aucune erreur de console en `http` ; manifeste à `start_url` relative et service
worker (hors-ligne) ; aucun débordement horizontal au téléphone.

## Suite, dans l'ordre

1. Lots 5, 6, 7, 8, dans cet ordre.
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
