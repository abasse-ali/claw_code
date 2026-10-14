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

