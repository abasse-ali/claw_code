# W1. Appli web de Claw sur GitHub Pages (comme Claude Code sur le web)

Demande de l'utilisateur, 14 octobre 2026 : « Je veux un github page de claw qui travaille sur
un repository comme avec claude et dispo sur mobile comme appli web » (capture de Claude Code
sur le web : dépôt, branche, modèle, « Décrivez une tâche ou posez une question »).

Choix de l'utilisateur : appli dans un **nouveau dépôt public `claw-web`** (aucun secret ni
code de Claw dedans, `claw_code` reste privé) ; l'agent travaille dans **GitHub Actions** ;
connexion par **jeton personnel** gardé dans le navigateur.

## Architecture

- **Appli** (`abasse-ali/claw-web`, GitHub Pages, installable sur le téléphone) : pages
  statiques, sans serveur. Choix du dépôt, de la branche et du modèle gratuit ; tâche ; liste
  des sessions ; conversation ; suivi en direct ; lien vers la pull request.
- **Moteur** (`claw_code`, privé) : workflow `.github/workflows/claw-web.yml`, lancé par
  l'appli (`workflow_dispatch` : dépôt, branche de départ, session, modèle). Il a le code de
  Claw et garde le binaire compilé en cache (recompilé seulement quand le code Rust change),
  clone le dépôt de la tâche avec le secret `CLAW_REPO_TOKEN`, travaille sur la branche
  `claw/session-<n>`, pousse, ouvre ou met à jour la pull request, et répond dans la session.
  Rien à installer dans les dépôts de l'utilisateur ; publics ou privés.
- **Sessions** : des issues privées de `claw_code` (étiquette `claw-web`). Le corps porte la
  tâche et ses réglages (dépôt, branche, modèle) ; chaque message de suivi est un commentaire ;
  le moteur publie un commentaire de progression mis à jour en direct, puis sa réponse.
- **D'un passage à l'autre** : le clone est neuf à chaque passage et `.claw/` n'est jamais
  poussé. La demande donne d'abord les mots de l'utilisateur (la tâche, ou les nouveaux
  messages), puis la conversation précédente et le dépôt dans des notes `[Claw]` : Claw ne
  décide le mode projet que sur ce qui précède la première ligne `[Claw]`. L'état du mode
  projet de la session (`.claw/project.json` : actif ou arrêté, plan relu, démonstration
  réussie) est gardé dans le cache d'Actions (`claw-web-state-<session>-...`). Sans cet état
  (nouvelle session sur un projet déjà fusionné), Claw reprend le mode projet de lui-même quand
  `SPEC.md` et un `PLAN.md` à jalons sont à la racine.
- **Arrêt** : « Arrêter » dans l'appli (deux appuis) annule le passage. Le script de l'agent,
  lancé par `exec`, reçoit le signal, arrête Claw et marque la progression « Claw a été
  arrêté ». Un passage inachevé (arrêt ou erreur) garde son travail sur la branche de la
  session sans ouvrir de pull request ; un message de suivi le reprend. Le titre du commit et
  de la pull request est le message traité, pas le premier message de la session.
- **Suppression** : « Supprimer » dans une session, « Choisir » dans la liste des sessions pour
  en supprimer plusieurs. La session (issue du moteur) est effacée par l'API GraphQL
  (`deleteIssue`) ; en option, sa branche dans le dépôt de la tâche (une pull request ouverte
  est fermée) et ses passages dans Actions. Une session au travail n'est pas supprimée. Si
  GitHub refuse au jeton d'effacer l'issue (l'autorisation que demande `deleteIssue` à un jeton
  à grain fin n'est pas documentée), la session est fermée et perd l'étiquette `claw-web` :
  elle quitte l'appli, et c'est dit.
- **Journal détaillé** (`journal.py`, `js/transcript.js`) : comme l'interface locale de Claw,
  chaque passage montre les messages de l'agent, ses réflexions (repliées), chaque outil
  dépliable (entrée et sortie des commandes, différences numérotées des écritures et
  modifications, liste de tâches cochée), les avis de Claw, puis modèle, requêtes et durée.
  Le moteur le publie toutes les 24 s, en bref (sorties et différences tronquées), dans des
  commentaires de la session : gzip puis base64 dans un commentaire HTML, invisible sur GitHub,
  60 000 caractères au plus par partie (un passage de 30 min : une partie de 42 000). L'appli
  ne relit que les commentaires modifiés (`since`) et garde les panneaux dépliés.
- **Modifications** : la vue « Modifications » d'une session montre les fichiers changés sur
  sa branche et leurs différences (API de comparaison de GitHub).
- **Diagnostic** : les actions et les avis de Claw vont au journal du job ; le journal complet
  de Claw (`claw.jsonl`, `claw.stderr`) est gardé 7 jours comme artefact du passage.
- **Secrets du moteur** : `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN`, posés par l'appli
  (chiffrés dans le navigateur avec la clé publique du dépôt) ou à la main.
- **Sécurité** : le jeton n'est jamais dans l'environnement de l'agent (clone puis adresse du
  dépôt sans jeton ; poussée après la fin de l'agent) ; les commentaires de progression passent
  par le jeton du job, limité à `claw_code` et périmé à la fin du job. L'appli ne parle qu'à
  `api.github.com` et `openrouter.ai` (politique de sécurité du contenu), et affiche le Markdown
  en échappant tout le HTML.

## Étapes

| Étape | Contenu | État |
|---|---|---|
| W1-1 | Moteur : workflow, préparation (session, dépôt, branche), agent avec progression, publication ; essai sur GitHub avec le faux modèle | fait le 14 octobre 2026 (6744ce0, 2ed50f0, fc2d0bb) |
| W1-2 | Appli : réglages (jeton, clé, secrets), accueil (dépôt, branche, modèle, tâche), sessions, conversation en direct, appli installable (manifeste, service worker, icônes) | fait le 14 octobre 2026 (dépôt `claw-web`, 8c0b5f2) |
| W1-3 | Publication : dépôt public `claw-web`, GitHub Pages, essai réel de bout en bout | en cours : Pages actif (https://abasse-ali.github.io/claw-web/) ; secrets `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN` posés (vérifications du moteur au vert dans l'appli) ; vrai modèle gratuit essayé (session 2) ; reste le premier essai de l'utilisateur sur un de ses dépôts |
| W1-4 | Retours de l'utilisateur : pas de zoom, comme une appli ; réglages sur leur propre page (`#/reglages`), plus à l'accueil | fait le 14 octobre 2026 (`claw-web` 1e04535) |
| W1-5 | Création de dépôt depuis le choix du dépôt (nom, description, public ou privé, `.gitignore`, premier commit) ; mode projet continu d'un passage à l'autre du moteur | fait le 14 octobre 2026 (`claw-web` 6e72294 ; moteur et Claw dans ce dépôt) |
| W1-32 | Lot 42 : refonte visuelle des réglages, demandée par l'utilisateur (page de 2 484 px faite de longs paragraphes d'aide). Cartes groupées à la façon d'iOS : compte GitHub (initiale, « Connecté », jeton et dépôt du moteur à déplier), moteur (synthèse « Moteur prêt » ou « N points à régler », dépliée seule s'il y a un point ; clés des modèles à déplier, clé déjà posée dite dans son champ, interrupteurs), sessions (« Machine de GitHub » ou « Ma machine », avec les machines en ligne ; serveur gratuit), thème en sélecteur à trois segments, requêtes gratuites, effacement du jeton ; aides repliées (« Autorisations du jeton » en pastilles, « Où créer ces clés », « Conseil »), ouvertes au premier lancement. Page de 1 610 px ; libellé « Ma machine » repris dans les notices et les scripts ; service worker v36 ; essai dans Chrome à 390 x 844 (22 contrôles : thèmes, choix de la machine, clés, synthèse, point à régler, premier lancement, connexion), et les essais des lots 31, 37 et 40 | fait le 20 octobre 2026 |
| W1-31 | Lot 41, premier démarrage du robot par l'utilisateur : secrets posés, puis « GitHub refuse » sur les variables (jeton à grain fin sans l'autorisation Variables, que la notice ne demandait pas). L'appli vérifie d'abord ce que fera le robot avec le jeton (une variable posée, un jeton d'inscription demandé au dépôt des sessions) et nomme l'autorisation qui manque (Variables ou Administration, et le dépôt) avant de poser le moindre secret ; notices de l'appli, du jeton et du serveur à jour ; service worker v35 ; essai dans Chrome à 390 x 844 (19 contrôles, dont les deux refus) | fait le 20 octobre 2026 |
| W1-30 | Lot 40 : robot de réessai du serveur Oracle, l'A1 restant introuvable à Marseille comme à Paris (« Out of host capacity »). Workflow `oracle-a1.yml` du dépôt public (toutes les 10 minutes, actif seulement quand l'appli l'allume) et `oracle/a1_robot.py` (kit d'Oracle 2.189.0) : A1 existante, sinon sous-réseau public et image Ubuntu 24.04 ARM trouvés seuls, jeton d'inscription neuf demandé au moment même, essais 2 OCPU et 12 Go puis 1 OCPU et 6 Go ; s'éteint (variable et workflow) une fois l'instance créée ou après une erreur définitive ; journal public sans identifiant ni jeton ; `test_a1_robot.py` (7 essais, et la requête construite avec les vrais modèles du kit). Appli : partie « Robot de réessai » (aperçu de configuration, fichier `.pem`, secrets chiffrés `OCI_CONFIG`, `OCI_KEY`, `CLAW_ADMIN_TOKEN`, variables, état, arrêt) ; service worker v34 ; essai dans Chrome à 390 x 844 (14 contrôles) | fait le 20 octobre 2026 |
| W1-29 | Lot 39 : le serveur de l'utilisateur à Marseille (une seule zone, « Out of capacity » pour l'A1) et sa machine `edt_stri` en E2.1.Micro (1 Go : trop petite pour Claw, et sans effet sur l'allocation ARM). `claw-cloud.sh` : architecture lue par `uname -m` (`dpkg` absent d'Oracle Linux), Docker installé par `apt` ou `dnf` (Docker CE ; Podman refusé), plus de `jq`, place disque vérifiée, `--shared` (moitié des processeurs et de la mémoire, priorité basse) et `--cpus` / `--memory`, gardés pour les mises à jour. Appli : commande pour un serveur déjà créé, conseils pour « Out of capacity » (1 OCPU et 6 Go, autres heures, Pay As You Go avec alerte de budget) ; service worker v33. Essais réels : Ubuntu 24.04 en `--shared` (14 min, limites appliquées), Oracle Linux 9 avec `--cpus 1 --memory 3g` (6 min 30, Docker CE installé), script lu sur l'entrée standard (`curl … \| sudo bash -s --`) | fait le 20 octobre 2026 |
| W1-28 | Lot 38, comportements de la session 13 : question sur le travail (« Tu fais quoi !? ») répondue en texte seul, sans reprise du travail (`harness::asks_about_the_work`, `CLAW_WORK_QUESTIONS` ; essai réel avec Nemotron 3 Ultra : une requête, 7 s, au lieu de 65 actions) ; « Continue » sans conversation qui précède : état du dépôt et suite proposée, aucun fichier modifié (`setup.bare_continue_note` ; essai réel : 3 requêtes, 14 s, rien de modifié) ; appels écrits au format `<function=…><parameter=…>` (Qwen3-Coder, Nemotron) changés en vrais appels ; vérification Python avec le venv du projet (plus d'installation dans le Python du système) ; remarques des contrôles dites venir de Claw, réponse finale au message de l'utilisateur ; modèle introuvable (404) mis de côté six heures | fait le 20 octobre 2026 |
| W1-27 | Lot 37 : sessions 24 h/24 sans GitHub Actions ni PC, sur un serveur gratuit d'Oracle Cloud (« Always Free » : 2 cœurs ARM, 12 Go, 200 Go ; Google Cloud e2-micro trop petit, Spaces Docker de Hugging Face devenus payants). Image de l'exécuteur en deux étapes (`runner` pour l'inscription dans l'heure du jeton, puis l'image complète), pour Intel/AMD et ARM (Node, exécuteur ARM ; Chromium de Playwright, Google Chrome n'existant pas pour Linux ARM) ; `claw-cloud.sh` (install, register, update, status, stop, start, remove), utilisable comme script de démarrage cloud-init ; étiquette `claw-pc` commune au PC et au serveur ; appli : « Sur ma machine », état de chaque exécuteur, « Serveur gratuit dans le nuage » (étapes chez Oracle, script préparé avec un jeton d'inscription neuf) ; workflow `runner-image.yml` (image construite et essayée sur Intel/AMD et sur vrai ARM, capture d'écran comprise) ; service worker v32 ; essai réel de `claw-cloud.sh` dans un Ubuntu 24.04 vierge (installation en 12 min 30, exécuteur en ligne), puis vraie session sur cet exécuteur avec le faux modèle (captures rangées dans `uploads/session-14/`) | fait le 20 octobre 2026 ; reste la création du serveur chez Oracle par l'utilisateur |
| W1-26 | Lot 36 : les captures d'écran de l'agent dans Claw Web. L'outil `Screenshot` garde aussi un aperçu du premier écran (160 px de large, `preview`) ; le moteur le glisse dans le journal et range la capture entière sur la branche `claw-web-fichiers` du dépôt des sessions, dans `uploads/session-<numéro>/` (`run.ShotStore`, branche créée au besoin, refus d'écriture noté une fois ; `test_shots.py`) ; workflow du moteur et de `claw-sessions` en `contents: write` (l'agent ne reçoit jamais ce jeton) ; appli : vignette avec pastille du nombre de problèmes, capture entière à faire défiler, effacée avec la session ; service worker v31 ; essai dans Chrome à 390 x 844 avec une vraie capture du site de `test_claw` (10 contrôles) | fait le 20 octobre 2026 |
| W1-25 | Lot 35, d'après la session 13 (« Continue », une session neuve sans l'historique de la 12) : Nemotron 3 Super introuvable chez NVIDIA (404 « Function not found for account ») ; son secours, Step 5 Preview, a écrit « user », recopié la demande et une fausse note de Claw, puis lancé la refonte du site qu'il y inventait. Claw écarte une réponse qui joue le rôle de l'utilisateur ou de Claw (`harness::invented_turn`), la redemande, puis passe au modèle suivant ; le flux stream-json garde chaque réponse jusqu'à ce qu'elle soit acceptée (une réponse écartée n'apparaît jamais dans Claw Web). Secours de NVIDIA : Super, Ultra, Lightning (essai réel de la même tâche : Ultra 126 s, Lightning 107 s) ; Step 5 Preview et gpt-oss 20B retirés. Photos trouvées par l'agent en vignettes cliquables dans le journal (outil `Photo` : `web_thumbnail`, `web_original` ; moteur : petite version téléchargée et glissée dans le journal, `test_journal.py` ; appli : pastilles de 52 px, photo en grand avec crédit et page d'origine, Content-Security-Policy ouverte à Flickr et Wikimedia) ; outils de design dits en clair ; noms de modèles raccourcis dans les avis ; service worker v30 ; essai dans Chrome à 390 x 844 avec le vrai journal de la session 13 et de vraies vignettes (8 contrôles) | fait le 20 octobre 2026 |
| W1-24 | Lot 34, d'après le dernier passage de la session 12 (57 minutes, 135 actions, réponse illisible) : le modèle se met au travail sans un mot (Nemotron 3 Super ne planifie que dans son raisonnement, en anglais, et ignorait la note du lot 33) : la requête suivante part sans outils (`tool_choice: none`), il annonce son plan en français, puis de nouveau après 20 étapes muettes ; le moteur lisait le texte et les outils dans le même événement alors que Claw les envoie séparément, d'où un plan jamais montré (`run.Progress`, `test_progress.py`) ; Claw envoie le texte d'une réponse dès sa fin. Vitesse : un gabarit de 52 Ko relu 28 fois, 62 000 jetons par requête : une lecture d'un fichier changé remplace la précédente, remarque à la troisième lecture entière d'un long fichier ; l'escalade de NVIDIA ne passe plus par Nemotron 3 Ultra. Lisibilité : consignes de la réponse finale (étiquettes dans la langue de l'utilisateur, pas de sélecteurs, pas de signature), listes imbriquées dans l'appli, bilan de branche en français, signature finale retirée, erreurs dites en clair, avis sans nom de réglage (`test_answer_text.py`) ; service worker v29 ; essai réel avec Nemotron 3 Super (annonce en français dès la deuxième requête) | fait le 20 octobre 2026 |
| W1-23 | Lot 33, d'après la session 12 (« Améliore le site », Nemotron 3 Ultra sur le PC : 42 minutes pour 16 actions, longues réponses coupées par NVIDIA et relancées jusqu'à six fois, pytest installé à grand-peine, plan annoncé en anglais) : la progression montre « Ce que Claw compte faire » puis « En ce moment » (`run.Progress`, `test_progress.py`) ; le moteur fixe `CLAW_LANGUAGE=français` ; Nemotron 3 Super en tête des modèles de NVIDIA, Ultra signalé lent ; secours du même fournisseur dans Claw ; image du PC avec pytest et `PIP_BREAK_SYSTEM_PACKAGES=1` (à reconstruire : `claw-pc.ps1 update`) ; service worker v28 | fait le 19 octobre 2026 |
| W1-22 | Lot 32 : le quota affiché suit le fournisseur du modèle choisi. Anneau du haut (OpenRouter : compte du jour ; Gemini : limite par jour relevée, « 20/jour » ; NVIDIA : « 40/min »), détail en trois parties (modèle choisi d'abord), ligne de quota sous chaque onglet du choix du modèle ; pied des réponses du moteur par fournisseur (`common.usage_lines`, requêtes du passage d'après `num_turns`, `test_usage_lines.py`) ; corrections de mise en page vues à la taille d'un iPhone (bouton « Ouvrir les réglages » sur le texte, détail qui ne défilait pas) ; service worker v27 ; essai dans Chrome à 390 x 844 (45 contrôles) | fait le 19 octobre 2026 |
| W1-21 | Lot 31 : modèles gratuits de Gemini (table des fournisseurs à clé de l'appli, secret `GEMINI_API_KEY` passé à Claw) ; suppression d'un dépôt (nom à retaper, moteur et code de Claw protégés, sessions concernées annoncées) et d'une branche (jamais celle par défaut) ; sessions sur le PC : exécuteur auto-hébergé `claw-pc` dans Docker Desktop (`.github/claw-web/pc`, `claw-pc.ps1`), entrée `runner` du moteur, dossier de travail vidé et `CLAW_CONFIG_HOME` dans le dossier temporaire du job, réglage « Sessions » et état de l'exécuteur dans l'appli ; choix du modèle en onglets OpenRouter, Google Gemini, NVIDIA (onglet sans clé : où la mettre ; recherche dans tous) ; service worker v25 ; essai dans Chrome avec un faux GitHub (38 contrôles). Né du forfait GitHub Actions d'octobre épuisé (dépôt privé des sessions) | fait le 19 octobre 2026 |
| W1-20 | Modèles de NVIDIA (lot 30) : clé NVIDIA facultative dans « Moteur » (secret `NVIDIA_API_KEY`, chiffré dans le navigateur, refusée sans `nvapi-`, jamais gardée sur l'appareil), ligne grise dans les vérifications quand elle manque, agents de NVIDIA juste après « Auto » dans le choix du modèle quand le moteur a le secret ; le workflow appelable passe `NVIDIA_API_KEY` à Claw (`claw-sessions` donne déjà tous ses secrets, `secrets: inherit`), jamais au faux modèle (`run.agent_env`, `test_agent_env.py`) ; service worker v23 ; essai dans Chrome avec un faux GitHub (13 contrôles) | fait le 18 octobre 2026 ; en ligne une fois les PR #1 et #2 fusionnées dans `main` |
| W1-19 | « Relancer » donne une nouvelle réponse au dernier message traité (ou à la tâche), et plus la tâche de départ ; elle porte la même marque `answered` et ses essais précédents sortent de l'historique de l'agent ; l'appli montre les passages d'un même message en pages (« ‹ Réponse 2/2 › », glissement horizontal), la plus récente d'abord ; la page choisie tient aux rafraîchissements ; les mails des sessions sont coupés (claw-sessions en « Ignorer ») ; essais `test_conversation.py` | fait le 16 octobre 2026 |
| W1-18 | Session 1 de claw-sessions (16 octobre) : chaque réponse s'affichait deux fois (fin du journal et réponse), Claw répétait ses tableaux d'état et sa liste de fonctions, annonçait une pull request à créer alors que la n° 2 était déjà fusionnée, prétendait que l'appli n'a pas besoin du jeton, et renvoyait l'utilisateur aux boutons de l'appli sans rien vérifier (Pages non activé, `test_claw` privé). Corrections : le journal ne répète plus la réponse ; le moteur lit l'état GitHub au départ (visibilité, branche, pull request, Pages et dernière construction) et le donne à l'agent ; commande `claw-github` (merge, pages, status, cancel), sans jeton : publish.py fusionne et met en ligne après la poussée et écrit l'issue sous la réponse ; consignes : faire soi-même, vérifier avant d'affirmer, ne pas répéter les réponses précédentes ; essais `test_on_github.py` (workflow `engine.yml`) | fait le 16 octobre 2026 |
| W1-17 | L'appli rejoint ce dépôt (dossier `claw-web`, historique gardé par `git subtree`), publiée par le workflow `pages.yml` à https://abasse-ali.github.io/claw_code/ ; l'ancien dépôt `abasse-ali/claw-web` ne fait plus que rediriger (page et service worker de nettoyage, partie après « # » gardée) ; les réglages restent, le domaine étant le même ; la CI Rust ignore `claw-web/` | fait le 15 octobre 2026 |
| W1-16 | Minutes de GitHub Actions : les sessions et le lancement du moteur passent dans un dépôt privé, `abasse-ali/claw-sessions` (son `claw-web.yml` appelle le workflow appelable `claw-web.yml` de ce dépôt, qui devient public : CI gratuite) ; session 12 transférée (issue 1 de claw-sessions) ; passages du moteur et leurs journaux effacés d'ici ; l'appli bascule d'elle-même vers le nouveau dépôt et lui donne ses secrets ; code de Claw lu avec `CLAW_REPO_TOKEN` quand il est privé | fait le 15 octobre 2026 |
| W1-15 | Menu à deux blocs, Sessions et Fichiers (dossiers d'une branche, dépôt et branche à changer, fichier ouvert en grand) ; branches : créer à partir d'une autre, renommer (les sessions suivent le nouveau nom) ; Équipe et Ultracode dans un menu déroulant à côté du trombone ; session : onglets Discussion et Modifications | fait le 15 octobre 2026 |
| W1-14 | Session en onglets (Discussion, Fichiers, Modifications) ; compteur sans bulle (anneau et nombre), nom à gauche ; appli installée sur iPhone : barre du haut et tiroir sous la bande floutée par iOS 26 (+18 px en portrait, à confirmer sur le téléphone) | fait le 15 octobre 2026 |
| W1-13 | Compteur des requêtes gratuites dans la barre du haut (toujours en vue, couleur selon ce qu'il reste, relu pendant les passages) ; « Fichiers » : dossiers et fichiers de la branche d'une session (fil d'Ariane, numéros de ligne, Markdown mis en forme, images) | fait le 15 octobre 2026 |
| W1-12 | D'après la session 12 (budget, nuit du 14 au 15 octobre) : le dernier passage s'arrêtait seul (`pkill -f "python3 app.py"` de l'agent tuait Claw, dont la ligne de commande portait la conversation) : `claw prompt -` lit la demande sur l'entrée standard ; quatre passages pour un message (appuis répétés sur « Relancer ») : bouton désactivé au premier appui, et le moteur ignore un passage dont les messages ont déjà leur réponse ; chaque réponse note le dernier message traité (un message envoyé pendant un passage a le sien) ; les commentaires du journal n'entrent plus dans l'historique (des données compressées y passaient) ; un message resté sans passage est signalé dans l'appli ; boutons « Fusionner » et « Mettre en ligne » (GitHub Pages : fusion puis main, ou branche telle quelle, dossier docs/ ou racine, dépôt privé signalé) ; l'agent sait qu'il ne peut ni fusionner ni régler GitHub et renvoie à ces boutons | fait le 15 octobre 2026 |
| W1-11 | Parité avec l'interface locale : options Équipe et Ultracode (puces, passées au moteur : CLAW_TEAM, CLAW_ULTRACODE) ; thème clair (sombre par défaut, clair, comme le téléphone) ; images jointes (réduites dans le navigateur, déposées sur la branche claw-web-fichiers du moteur, jointes à Claw par le moteur, vignettes, effacées avec la session) | fait le 15 octobre 2026 ; essayés sur GitHub avec le faux modèle (Ultracode : dossier préparé ; image : téléchargée et jointe) |
| W1-10 | Modèles gratuits classés comme Claw dans le choix du modèle (le plus puissant en tête, raisons de chaque rang ; « Auto » = le premier, comme le moteur) ; requêtes gratuites du jour dans l'appli (clé OpenRouter gardée sur l'appareil, en option) et dans chaque réponse du moteur (ce que le passage a pris, ce qu'il reste) | fait le 15 octobre 2026 ; classement de l'appli identique à la chaîne suivie par le moteur dans la session 5 |
| W1-9 | Numéros de session propres à l'appli (le plus grand plus un, 1 quand il n'y en a plus) et branche notée dans la session (`claw/session-<numéro>`, suffixe si une ancienne branche du même nom reste) ; le moteur prend cette branche ; le numéro d'issue (qui ne recule jamais) reste interne | fait le 15 octobre 2026 |
| W1-8 | Interface « comme Claw » : journal détaillé de chaque passage (messages, réflexions, outils avec entrée et sortie, différences, tâches) et vue des modifications de la branche ; outil de capture adapté à Edge 155 (port lu dans `DevToolsActivePort`) | fait le 15 octobre 2026 (moteur 4e95875 ; `claw-web` 7a4b497) ; essayé sur le vrai journal de la session 12 (237 événements) et sur GitHub avec le faux modèle (journal lu pendant puis après le passage) |
| W1-7 | Supprimer des sessions (une ou plusieurs, avec branches et passages) ; d'après les sessions 5 et 9 : vérification d'un jalon limitée à 3 échecs par tour (45 relances dans la session 9), commandes `Verify` mal lues (`--hors-ligne` lancé seul), plan sans lignes `Status:` non signalé (jalons de la session 5 jamais vérifiés), passage arrêté sans conclusion présenté comme une erreur | fait le 14 octobre 2026 (`claw-web` bbef23e ; Claw et moteur dans ce dépôt) |
| W1-6 | Retours du premier essai de l'utilisateur (session 5, `test_claw`) : mode projet pas activé par « Crée un Comparateur de prix... » (objet inconnu de Claw) ; aucun moyen d'arrêter depuis l'appli ; à l'annulation, Claw continuait pendant la publication, qui ouvrait une pull request pour un travail inachevé, sous le titre du premier message ; chemins absolus du job dans la progression ; puces de la zone de saisie sur les boutons de la session ; avertissement Node.js 20 sur chaque job | fait le 14 octobre 2026 (d422f5a, be1c522, a59e3a2 ; `claw-web` 89e8438) |

## Essais

- Moteur avec le faux modèle (session 1, sur `claw_code` lui-même) : premier passage 4 min 26 s
  compilation comprise, progression mise à jour en direct, réponse publiée ; la publication
  échouait sur `git add -A -- . ':(exclude).claw'` (git refuse de nommer un chemin ignoré) :
  `.claw/` étant exclu par `.git/info/exclude`, un simple `git add -A` suffit. Message de suivi
  sur la même session : binaire en cache, fini en 3 s, modifications non publiées faute de
  `CLAW_REPO_TOKEN` (dit dans la réponse). Les caches des tests (`__pycache__`...) sont
  maintenant exclus du commit.
- Appli au format téléphone (Edge sans fenêtre piloté par le protocole DevTools, émulation
  390 x 844 tactile ; `--headless=new` impose sinon une largeur minimale et coupe la capture) :
  premier lancement, accueil connecté (sessions récentes, puces dépôt, branche, modèle),
  session (messages, progression, réponses, actions). Chiffrement des secrets : BLAKE2b
  identique à Python sur 33 cas, aller-retour de la sealed box. Rendu Markdown : HTML et liens
  `javascript:` neutralisés.
- Continuité du mode projet (6c62306, e48d59d), sur GitHub avec le faux modèle, session 4
  (tâche en mode projet, sans consigne d'écriture) : premier passage en 21 s (binaire en
  cache), mode projet activé, état mis en cache ; message de suivi : « État du mode projet
  repris du passage précédent », mode projet poursuivi ; aucun fichier modifié, rien publié.
  Un passage au faux modèle ne reçoit plus aucune clé réelle (relecteurs compris).
- Incident pendant cet essai : relancée avec le faux modèle, la session 1 portait dans sa tâche
  une consigne d'écriture (`[mock:write ...]`) ; `CLAW_REPO_TOKEN` étant maintenant posé, le
  moteur a poussé `claw/session-1` et ouvert la pull request n° 3 dans `claw_code` (le fichier
  du faux modèle et quatre fichiers `.port_sessions/` laissés par les essais Python du dépôt).
  Pull request fermée, branche supprimée, sa CI arrêtée. Les essais du moteur se font
  désormais sur une session dont la tâche ne fait rien écrire.
- Premier essai de l'utilisateur (session 5, `test_claw`, modèle Auto) : une question, puis
  « Crée un Comparateur de prix... » ; mode projet pas activé, travail commencé sans cahier des
  charges ; `nemotron-3-ultra` a lâché (504) et la requête est passée à `laguna-s-2.1`, comme
  prévu quand personne ne peut répondre ; passage annulé depuis GitHub au bout de 9 min ;
  Claw a continué pendant la publication, qui a ouvert la pull request n° 1 de `test_claw`
  (sept fichiers du début du backend, titre « Donne des idées de projet web »). Corrections :
  W1-6.
- Arrêt, sur GitHub avec le faux modèle (sessions 6 et 7, commande `sleep 100` lancée par
  l'agent, annulée en cours) : carte « Claw a été arrêté après 1 min 00 s (passage annulé) »,
  réponse « La tâche a été arrêtée », chemin montré `ls . > /dev/null && sleep 100`, titre tiré
  du message, journal de Claw en artefact, aucune pull request, plus d'avertissement Node.js 20.
  À la session 6, le `sleep` survivait à Claw jusqu'au nettoyage du job : l'arbre des
  processus de Claw est maintenant arrêté (session 7 : aucun orphelin). Les deux passages
  annulés n'avaient pas gardé le binaire compilé (sauvegarde d'actions/cache en fin de job
  réussi seulement) : il est gardé dès la compilation (session 8 : « Cache saved » juste
  après). Appli : vue de session au format téléphone avec un faux GitHub (boutons 28 px
  au-dessus de la zone de saisie, aucun débordement, « Arrêter » en deux appuis).
- Sessions 5 et 9 de l'utilisateur (14 octobre, 15 h 22), lancées avec le même texte (deux
  options de reprise copiées de ma réponse) : « mode projet » y a activé le mode projet.
  Session 5 (`test_claw`, 33 min, 100 requêtes, bascules de modèle sur 429 et 503) : plan sans
  aucune ligne `Status:`, donc aucun jalon vérifié par Claw, alors que la réponse annonçait
  M1 à M6 faits ; 56 fichiers sur `claw/session-5` (pull request n° 1 mise à jour). Session 9
  (`edt_stri`, dépôt réel et public de l'utilisateur, 40 min) : `SPEC.md`, `PLAN.md`,
  `DECISIONS.md` écrits pour le projet existant, M1 renvoyé 45 fois (Poppler absent de la
  machine du job, finalement téléchargé par l'agent), `--hors-ligne` lancé comme commande ;
  limite de 100 requêtes atteinte sans conclusion ; travail gardé sur `claw/session-9` sans
  pull request (`donnees/journal.csv` y est modifié par les commandes lancées). Corrections : W1-7.
- Suppression, appli au format téléphone avec un faux GitHub : une session (branche, passage,
  session effacés, retour à l'accueil) ; trois cochées dont une au travail (exclue et dite),
  pull request n° 1 annoncée puis fermée, effacement refusé pour l'une (repli : fermée, sans
  étiquette, dit). Effacement réel par l'API GraphQL essayé sur une issue jetable créée pour
  l'occasion (n° 10, effacée : « 410 Gone » ensuite).

