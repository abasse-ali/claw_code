# Claw Code

Claw Code est une CLI d'agent de codage local implémentée en Rust sûr. Elle est **inspirée de Claude Code** et développée comme une **implémentation en chambre blanche** : elle vise une expérience d'agent local solide, mais elle n'est **pas** un portage direct ni une copie de Claude Code.

L'espace de travail Rust est la surface produit principale actuelle. Le binaire `claw` fournit des sessions interactives, des prompts ponctuels, des outils conscients de l'espace de travail, des workflows d'agents locaux et un fonctionnement compatible avec les plugins depuis un unique espace de travail.

## Statut actuel

- **Version :** `0.1.0`
- **Étape de publication :** publication initiale publique, distribution par construction depuis les sources
- **Implémentation principale :** espace de travail Rust dans ce dépôt
- **Plateformes ciblées :** stations de travail développeur macOS et Linux

## Installer, construire et exécuter

### Prérequis

- Chaîne d'outils Rust stable
- Cargo
- Identifiants du fournisseur pour le modèle que vous souhaitez utiliser

### Authentification

Modèles compatibles Anthropic :

```bash
export ANTHROPIC_API_KEY="..."
# Facultatif lors de l'utilisation d'un point de terminaison compatible
export ANTHROPIC_BASE_URL="https://api.anthropic.com"
```

Modèles Grok :

```bash
export XAI_API_KEY="..."
# Facultatif lors de l'utilisation d'un point de terminaison compatible
export XAI_BASE_URL="https://api.x.ai"
```

N'importe quel modèle via OpenRouter (Claude, GPT, Gemini, Qwen, DeepSeek, Grok…) :

```bash
export OPENROUTER_API_KEY="sk-or-..."
claw --model qwen/qwen3-coder
# ou, pour en faire le modèle par défaut :
export CLAW_MODEL="anthropic/claude-sonnet-4.5"
```

Tout identifiant de la forme `fournisseur/modèle` (tel qu'affiché sur openrouter.ai/models)
est envoyé à OpenRouter. Le modèle par défaut peut aussi être fixé avec la clé `model`
de `.claw.json`. Le modèle choisi doit prendre en charge l'appel d'outils (*tool calling*).

| Variable | Rôle |
|---|---|
| `OPENROUTER_API_KEY` | Clé OpenRouter (obligatoire) |
| `OPENROUTER_BASE_URL` | Point de terminaison (défaut `https://openrouter.ai/api/v1`) |
| `OPENROUTER_APP_TITLE` / `OPENROUTER_APP_URL` | Attribution de l'application dans OpenRouter (facultatif) |
| `CLAW_MODEL` | Modèle de départ (prioritaire sur `.claw.json`) |
| `CLAW_PROVIDER` | Force le fournisseur : `anthropic`, `openrouter`, `openai`, `xai` |
| `CLAW_MAX_TOKENS` | Plafond de jetons de sortie (défaut 16 384 pour OpenRouter) |
| `CLAW_IDLE_TIMEOUT` | Secondes sans rien recevoir du serveur avant d'abandonner une requête à un modèle, qui est alors relancée. Défaut : 300 pour un serveur distant, aucune limite pour un serveur local (un modèle local peut rester muet plusieurs minutes sur une machine lente). 0 : attendre sans limite partout |

**Windows : « clé manquante » alors que la clé est définie.** Un programme garde l'environnement
reçu à son démarrage : VS Code ouvert par le clic droit « Ouvrir avec Code », un terminal resté
ouvert ou une tâche planifiée peuvent ignorer une variable créée depuis. Claw relit donc au
démarrage les variables de votre compte dans le registre (`HKCU\Environment`, puis celles de la
machine si aucune clé n'est trouvée) et adopte celles qui lui servent et qui manquent : clés
(`*_API_KEY`, `*_AUTH_TOKEN`), adresses (`*_BASE_URL`), `CLAW_*`, `OLLAMA_*`. Une variable déjà
présente n'est jamais remplacée, et aucune valeur n'est affichée. `claw doctor` le signale
(« clé lue dans les variables du compte Windows ») ; `CLAW_NO_USER_ENV=1` coupe cette reprise.

Sous Windows, l'outil `bash` et les hooks utilisent le bash de Git for Windows,
détecté automatiquement ; `CLAW_SHELL` permet d'indiquer un autre `bash.exe`. Les modèles y
écrivent volontiers des commandes pour cmd, que bash ne comprend pas. Claw le leur dit dans ses
consignes et rattrape les cas courants : un chemin `D:\dossier\fichier` écrit hors guillemets
devient `D:/dossier/fichier`, `cd /d chemin` devient `cd chemin`, et une commande qui échoue
avec un chemin relatif en barres obliques inverses (`dossier\fichier`) revient au modèle avec un
rappel de la syntaxe (`CLAW_PATH_HELP=0` coupe ce rappel).

### Interface web : `claw serve`

```bash
claw serve                 # ouvre http://127.0.0.1:8787/?token=… dans le navigateur
claw serve --port 9000 --no-open
```

Même agent que dans le terminal (modèle, permissions, MCP, sous-agents, sessions), dans une page
sombre ou claire (thème du système, de VS Code ou choisi) :

- réponses en direct (seule la fin du message est re-rendue pendant le flux) avec Markdown,
  tableaux et code coloré avec bouton « Copier » ;
- **frise des étapes de l'agent** : une ligne par outil (icône, cible, durée, `+3 −1` pour une
  modification), dépliable sur la sortie, le contenu lu ou la liste de tâches ; la barre
  d'activité au-dessus de la saisie dit ce que fait l'agent ;
- **modifications en deux colonnes, AVANT et APRÈS**, ouvertes d'emblée et limitées aux
  **seules lignes changées** : chaque ligne d'avant (en rouge) fait face à celle qui la remplace
  (en vert), la partie changée de la ligne est surlignée, et « ⋯ » sépare deux passages
  éloignés. Les numéros de ligne sont ceux du fichier : à l'arrivée du résultat, Claw compare le
  fichier entier d'avant et d'après. Une réécriture complète d'un fichier existant ne montre aussi
  que ses lignes changées ; un nouveau fichier s'affiche en entier, numéroté. Au-delà de
  14 lignes, la vue se replie (« Afficher les N lignes changées »). En panneau étroit, l'après
  passe sous l'avant ;
- **flèche du haut dans la zone de saisie** : reprend vos demandes précédentes, de la plus récente
  à la plus ancienne (100 gardées d'une session à l'autre) ; la flèche du bas redescend jusqu'au
  brouillon en cours. Dans un texte de plusieurs lignes, les flèches déplacent le curseur tant
  qu'il n'est pas sur la première (ou la dernière) ligne ;
- **une seule zone pour tout ce qui attend votre réponse**, épinglée au-dessus de la saisie et
  toujours visible : rien ne reste dans la conversation une fois répondu.
  - **Autorisations** : `Entrée` autoriser, `A` toujours, `Échap` refuser, ou « Refuser avec une
    consigne ». Seule une règle « Toujours » enregistrée laisse une ligne dans le fil.
  - **Questions de l'agent**, comme dans Claude Code : quand il a besoin d'un choix ou d'une
    validation, l'agent appelle l'outil `AskUserQuestion` (1 à 4 questions, 2 à 4 propositions
    chacune, la recommandée en premier). Un clic sur une proposition répond et passe à la
    suite ; plusieurs réponses peuvent être cochées quand la question le permet ; un champ
    « Autre réponse » accepte ce que vous tapez ; « Ne pas répondre » laisse l'agent décider.
    Dans le terminal, les mêmes questions s'affichent en menus. L'outil n'est proposé au modèle
    que si quelqu'un peut répondre. Un petit modèle écrit volontiers ses questions à la fin de
    sa réponse au lieu d'appeler l'outil : quand une réponse se termine par au moins deux
    questions, Claw le renvoie une fois les poser avec le menu, puis le tour continue avec vos
    réponses. La proposition qu'il venait d'écrire reste affichée. Cela coûte une requête de
    plus ; `CLAW_QUESTIONS=0` le coupe.
  - **Modèle qui ne répond plus** : compte à rebours de l'essai automatique, changement de modèle.
- **défilement** : le fil suit les nouveaux messages tant que vous ne remontez pas. Dès que vous
  faites défiler vers le haut, plus rien ne vous ramène en bas, même pendant que l'agent écrit ;
  « Derniers messages » y retourne ;
- **pied de page** : la taille du contexte (ce que le modèle relit à chaque requête), le total
  échangé depuis le début de la session restant au survol ;
- **réponses** : un lien vers un fichier du projet s'affiche par son nom (chemin au survol), les
  tableaux défilent en panneau étroit au lieu de couper les mots, et une remarque que Claw a
  adressée au modèle apparaît à part, sous le résultat de l'outil, et non dans sa sortie ;
- palette `Ctrl+K` (actions, modèles, modes, commandes), suggestions `/`, images collées,
  glissées ou jointes, modèle et mode en un clic ;
- **crédits OpenRouter en direct** : solde et requêtes gratuites du jour sont les chiffres
  d'OpenRouter lui-même (`GET /credits` et `GET /key`), décomptés à chaque requête de Claw puis
  relus toutes les 20 s tant qu'une page est ouverte. Un clic ouvre le détail : heure de la
  dernière lecture, **requêtes de Claw par modèle** aujourd'hui, et **rythme** (« à ce rythme,
  quota épuisé vers 18:05 », d'après la dernière heure). Sous 100 requêtes gratuites restantes
  (puis 20, puis 0) la pastille passe à l'orange et un message prévient, une fois par seuil ;
- **proches de Claude** : la première catégorie du menu range les modèles gratuits du plus
  proche au moins proche du dernier **Claude Opus** ou du dernier **Claude Fable** du catalogue
  (au choix), avec un pourcentage et ce qui les rapproche ou les éloigne ; la fiche d'un modèle
  détaille les neuf critères : appels d'outils, raisonnement, lecture d'images, contexte,
  réponse maximale, orientation code et agents de sa description, rang en programmation chez
  OpenRouter, spécialités en commun, mesures de Claw. C'est une ressemblance de profil, pas une
  égalité d'intelligence : « Mesurer ce modèle » vérifie ses capacités réelles. Dans le terminal :
  `/models claude`.
  Sous les gratuits, **« Payants proches ou mieux classés »** liste au plus huit modèles payants
  par référence : ceux qui ressemblent à Claude au moins autant que le meilleur gratuit (60 % au
  minimum) et ceux qui le devancent au classement en programmation d'OpenRouter (un classement
  d'usage, pas d'intelligence), avec leur prix par million de jetons. Ils sont là pour
  information : Claw ne choisit jamais un payant de lui-même (ni au démarrage, ni en secours, ni
  pour l'escalade ou les avis). Un payant ne sert que si vous cliquez dessus puis confirmez son
  prix, ou si vous tapez `/model <id>` ;
- **menu des modèles** : nom, identifiant réel et **spécialités** de chaque modèle gratuit, avec
  une **catégorie par spécialité** (Programmation, Images, Raisonnement, Apprentissage et
  recherche, Audio et vidéo, Long contexte, Traduction, Santé, Marketing… ; les catégories sans
  modèle gratuit sont repliées sous « Autres catégories »). Une catégorie liste les modèles
  gratuits, puis les payants les plus utilisés chez OpenRouter, marqués « payant » avec leur
  prix et une confirmation avant usage. Le bouton « Rechercher les modèles gratuits les plus
  puissants » relit le catalogue et les classements d'OpenRouter (sinon gardés 24 h) et signale
  les nouveaux venus : ce sont des lectures publiques, faites sans la clé, qui **ne consomment
  aucune requête gratuite** (seuls les messages à un modèle et « Mesurer » en consomment) ;
  l'étoile épingle un modèle en **favori**, en haut du menu (gardé dans
  `~/.claw/favorite-models.json`, commun au navigateur et à VS Code) ; le bouton d'information
  ouvre sa fiche (fournisseur, contexte, réponse maximale, prix, rang, puissance estimée,
  mesures de `/models bench`, disponibilité, lien vers OpenRouter) d'où l'on peut le **mesurer**
  d'un clic, sur les trois tâches de code ou sur les tâches de raisonnement, puis revenir à la
  liste par « Modèles » (même catégorie, même recherche, même position) ;
- **sessions** : nouvelle session, reprise d'une session précédente, **recherche** dans les
  titres et dans le texte des messages (l'extrait trouvé s'affiche), **titre** modifiable
  (crayon), effacement d'une session (corbeille) ou de toutes les précédentes, après
  confirmation ; la session en cours ne s'efface pas ;
- **agents** (bouton à côté de « Sessions », comme le panneau des sessions en arrière-plan de
  Claude Code) : les sessions en arrière-plan de la machine avec leur état et leur dernière
  action ; une tâche tapée en haut en lance une nouvelle dans son propre worktree, avec le modèle
  et le mode de la page (`!commande` pour une commande shell). Un clic ouvre la session : ses
  dernières sorties en direct, sa question en cours avec « Autoriser », « Toujours » ou
  « Refuser » pour une permission, et un champ pour lui répondre ou lui donner une nouvelle
  demande ; elle s'arrête ou se retire de la liste d'un clic. Le même panneau montre les tâches
  planifiées de la conversation (corbeille pour en supprimer une), ses Monitors (bouton pour les
  arrêter), ses sous-tâches et les sessions joignables par message. Une pastille orange compte
  les sessions qui attendent une réponse et les messages retenus ;
- diagrammes Mermaid à la demande ; commandes `/review`, `/audit`, `/archi`, `/adr`, `/fix`,
  `/models`, `/credits`, `/mcp`, `/cost`, `/compact`…
- **entre deux messages**, la page reçoit comme le terminal les passages des tâches planifiées,
  les lignes des Monitors, les messages des autres sessions et les résultats des sous-tâches :
  chacun s'annonce dans le fil (`↻ Tâche planifiée…`, `› Monitor : …`, `› Message de @…`) puis
  l'agent y répond. La page est joignable par les autres sessions sous le nom du dossier suivi de
  `-web` ;
- les commandes de travail continu et de revue marchent aussi dans la page : `/goal` et `/loop`
  (le bouton Arrêter met en pause l'objectif ; `/loop stop` arrête la boucle), `/tasks`,
  `/subtask`, `/bg` (la conversation part en arrière-plan et la page repart d'une conversation
  vide), `/fork`, `/peers`, `/workflows`, `/deep-research`, `/btw`, `/recap`, `/copy`
  (copie dans le presse-papiers du navigateur), `/rename`, `/output-style`, `/schedule`,
  `/install-github-action`, `/security-review`, `/code-review`, `/pr-comments`, `/branch`,
  `/worktree`, `/add-dir`, `/permissions allow|deny|ask|remove <règle>` ;

Sécurité : écoute sur `127.0.0.1` uniquement (sauf `--host`), jeton secret aléatoire à chaque
lancement exigé par toute l'API, contrôle des en-têtes `Host` et `Origin` (DNS rebinding,
requêtes d'autres sites), politique CSP stricte ; Mermaid est chargé depuis jsDelivr en version
figée avec vérification d'intégrité (SRI). L'adresse affichée contient le jeton : ne la partagez pas.

### Modèles gratuits pour le code : `/models`

`/models` (ou `claw models` dans le terminal) liste uniquement les modèles **gratuits**
d'OpenRouter qui **savent appeler des outils** et sont **bien placés en code**, classés selon
des critères vérifiables : rang dans la catégorie « programming » d'OpenRouter, spécialisation
et benchmarks de code cités par le modèle, puis taille. Chaque ligne indique pourquoi le modèle
est retenu ; avec `OPENROUTER_API_KEY`, le quota gratuit du jour est affiché.

- `/models refresh` recharge le catalogue (sinon mis en cache 24 h dans `~/.claw/cache/`),
  `/models <recherche>` filtre, `/model <id>` change de modèle. Lire le catalogue ne consomme
  aucune requête gratuite : c'est une page publique d'OpenRouter, lue sans la clé.
- **Spécialités** : chaque modèle porte celles que des faits vérifiables justifient : son rang
  dans les douze catégories d'usage d'OpenRouter (programmation, apprentissage et recherche,
  sciences, technologie, traduction, finance, santé, droit, marketing, référencement, jeu de
  rôle, culture générale), ce qu'il sait lire (images, son, vidéo), un contexte d'au moins
  500 000 jetons, et ce que sa description met en avant (code, raisonnement, recherche,
  documents). `/models` les rappelle après « aussi : ».
- Sans modèle configuré et avec seulement une clé OpenRouter, l'agent démarre sur le premier
  modèle de ce classement. `"model": "free"` (ou `--model free`, `/model free`) demande la même
  chose explicitement : **le modèle gratuit le plus puissant du moment**, choisi à chaque
  lancement, plutôt qu'un identifiant figé qui vieillit avec le catalogue.
- Le quota gratuit d'OpenRouter (1 000 requêtes par jour après 10 $ de crédits achetés, 50
  sinon ; 20 par minute) est compté **par compte**, sur la journée UTC : une nouvelle clé API
  n'y change rien, OpenRouter l'écrit dans sa documentation. Pour continuer une fois le quota
  atteint : un modèle local (`ollama:…`) ou un autre fournisseur qui a son propre quota gratuit
  (`gemini:`, `nvidia:`, `groq:`), chacun avec sa clé. Le plan gratuit de Mistral ne permet plus de créer
  de clé API (constaté le 10 octobre 2026) : `mistral:` demande un plan payant.
- **La clé Google (`GEMINI_API_KEY`)** ne sert que si vous la mettez au travail : rien ne
  l'utilise d'office. `/model gemini:<modèle>` pour une session ; ou, pour épargner le quota
  d'OpenRouter, les tâches annexes sur celui de Google : `"advisorModels": ["gemini:<modèle>"]`
  (relecture, conseil, second avis) et `"imageReader": "gemini:<modèle>"` (description
  d'images). Avant cela, vérifiez dans Google AI Studio que le projet de la clé est en offre
  gratuite : un projet dont la facturation est activée paie chaque requête. Les modèles gratuits
  et leurs limites (par minute et par jour, par projet) s'y affichent ; en offre gratuite, Google
  peut se servir des requêtes pour améliorer ses produits.
- **Gemini 3 et ses signatures de réflexion** : chaque appel d'outil d'un modèle Gemini 3 arrive
  avec une signature que Google exige de retrouver dans la requête suivante (sans elle : erreur
  400 dès la deuxième étape). Claw la garde et la renvoie avec son appel, au seul serveur et au
  seul modèle qui l'ont écrite : un autre fournisseur (Mistral, Groq…) refuserait ce champ, et un
  autre modèle Gemini n'accepte pas la signature d'un autre. Un appel sans signature de ce modèle
  (session rechargée, appel écrit par un autre modèle de l'équipe) reçoit la valeur de
  remplacement que Google documente. Sur une erreur 429 de Google, Claw attend le délai qu'il
  annonce (`retryDelay`, unité comprise). Un quota du jour épuisé (`…PerDay…`) n'est ni attendu
  ni réessayé, même si Google y joint un court délai : le modèle est mis de côté jusqu'à minuit,
  heure du Pacifique (9 h à Paris), l'heure où Google remet ses quotas à zéro.
- **Modèles rangés par fournisseur** : le menu des modèles de la page a, à la suite, les
  catégories **OpenRouter** (ses modèles gratuits qui appellent les outils), **Google Gemini** et
  **NVIDIA** (avec leur clé), en plus des catégories par spécialité. Claw Web range son choix
  du modèle de la même façon, en onglets.
- **Secours du même fournisseur** : sans `fallbackModels`, un modèle de NVIDIA a pour secours les
  autres agents de NVIDIA (Nemotron 3 Super, puis Nemotron 3 Ultra, puis Nemotron 3.5 Lightning),
  et un modèle Gemini les autres modèles de l'offre gratuite de Google, chacun
  avec son propre quota du jour ; puis deux modèles gratuits d'OpenRouter en dernier recours. Vu
  dans une session de Claw Web : Nemotron 3 Ultra seul, dont NVIDIA coupait les longues réponses,
  était relancé jusqu'à six fois par étape (42 minutes pour 16 actions). Nemotron 3 Ultra, le plus
  puissant, est signalé lent dans `/models nvidia` et dans le menu des modèles.
- **Langue des réponses** : `CLAW_LANGUAGE` (ex. `français`) l'emporte sur le réglage `language` ;
  Claw Web la fixe à `français`, car son moteur entoure la demande de notes en anglais.
- **Commandes installées par `pip`** : dans le bac à sable de Linux, `pip install --user` range ses
  commandes dans le dossier personnel du bac à sable (`.claw/sandbox/home/.local/bin`), maintenant
  en tête du PATH : l'agent ne corrige plus le PATH à la main.
- **Quota du fournisseur du modèle choisi** : la pastille des crédits de la page suit le modèle.
  Avec OpenRouter, elle donne son compte du jour. Avec Gemini, elle donne les requêtes de Claw sur
  cette machine depuis minuit, heure du Pacifique (l'heure où Google remet ses quotas à zéro), sur
  la limite relevée du modèle (« 3 / 20 Gemini ») ; Google ne donne pas le compte en direct, le
  compte exact est sur AI Studio. Avec NVIDIA, elle donne sa limite par minute (« 40 / min
  NVIDIA »), sans quota par jour. Le détail (« Quotas et crédits ») montre le fournisseur du
  modèle choisi d'abord, même sans clé OpenRouter. Le journal des requêtes de Claw compte
  maintenant celles de Gemini et de NVIDIA, à part du compte d'OpenRouter.
- **Choisir un modèle Gemini** : `/models gemini` liste les modèles de l'offre gratuite de Google
  que votre clé ouvre (une simple lecture du catalogue de Google, aucune génération), et la page
  les montre dans la catégorie **Google Gemini** du menu des modèles. Gratuits d'après la page
  des tarifs de Google (relevée le 10 octobre 2026) : `gemini-3.8-flash`, `gemini-3.6-flash`,
  `gemini-3.5-flash-lite`, `gemini-3.1-flash-lite`, `gemini-2.5-pro`, `gemini-2.5-flash`,
  `gemini-2.5-flash-lite`. `gemini-3.1-pro-preview` est payant et n'est pas proposé. Exemple :
  `/model gemini:gemini-3.8-flash`.
- **NVIDIA (`NVIDIA_API_KEY`, préfixe `nvidia:`)** : les modèles de build.nvidia.com sont
  gratuits pour les membres du programme pour développeurs de NVIDIA (inscription gratuite, sans
  carte), avec environ 40 requêtes par minute pour le compte (observé par les utilisateurs, non
  publié) et aucun quota par jour annoncé ; un usage en production demande une licence. La clé se
  crée sur build.nvidia.com (profil, Settings, API Keys) et commence par `nvapi-`. `/models nvidia`
  liste les modèles essayés avec Claw que le catalogue de NVIDIA propose, et la page les montre
  dans la catégorie **NVIDIA** du menu des modèles. Agents proposés, revus le 20 octobre 2026 :
  `nvidia/nemotron-3-super-120b-a12b`, `nvidia/nemotron-3-ultra-550b-a55b`,
  `nvidia/nemotron-3.5-lightning-30b-a3b`. Ce jour-là, Super était introuvable (erreur 404
  « Function not found for account »), et la même petite tâche a été faite pour de vrai par les
  deux autres : Ultra en 126 s, tout en français, avec un compte rendu net ; Lightning en 107 s,
  avec une correction de plus et des phrases en anglais. Step 5 Preview et gpt-oss 20B ne sont
  plus proposés : le premier, en secours d'une session de Claw Web, a écrit de faux messages de
  l'utilisateur puis suivi les tâches qu'il y inventait ; le second échouait. Le catalogue en
  compte 81 ; d'autres font attendre leur première réponse plus de 4 minutes (GLM 5.3, `DeepSeek`
  V4.1 Flash) ou répondent 404 selon le compte (Kimi K2.6). Un modèle qui ne lit pas les images refuse
  une requête qui en porte une (erreur 400) : Claw sait lesquels les lisent et fait décrire les
  images aux autres. `nvidia/nemotron-3.5-super-vl-120b-a12b` lit les images mais échoue dès
  qu'on lui donne des outils : avec la clé, il décrit les captures et juge les designs avant les
  lecteurs d'OpenRouter (une capture de site lue en cinq secondes à l'essai, sans quota par
  jour), sans jamais être l'agent. `/models bench --design nvidia:` ne mesure que les modèles de
  NVIDIA. Un modèle de NVIDIA bloqué passe la main à Nemotron 3 Super, sur le même compte,
  plutôt qu'à un modèle d'OpenRouter ; jamais à Nemotron 3 Ultra, lent sur l'offre gratuite, qui
  coupait ses longues réponses. Nemotron 3 Super bloqué passe à l'escalade habituelle (le modèle
  gratuit le plus puissant d'OpenRouter, s'il en reste un de disponible). Exemple :
  `/model nvidia:nvidia/nemotron-3-super-120b-a12b`.
- La limite de jetons de sortie suit le catalogue (sortie maximale et contexte du modèle) ;
  `/cost` indique le coût estimé (0 $ pour un modèle gratuit).
- Chaque étape de l'agent consomme une requête du quota gratuit ; en cas de limite atteinte
  (erreur 429), la session continue et un conseil s'affiche.

### Permissions

| Mode (`--permission-mode`, `/permissions`) | Alias Claude Code | Comportement |
|---|---|---|
| `prompt` (défaut) | `default` | Lecture libre ; demande avant chaque modification ou commande |
| `read-only` | `plan` | Mode plan : lecture et commandes sans effet uniquement ; l'agent propose un plan |
| `workspace-write` | `acceptEdits` | Modifie sans demander les fichiers du projet (et des dossiers ajoutés) ; demande avant les commandes et avant d'écrire ailleurs |
| `dont-ask` | `dontAsk` | Ne demande jamais : lecture et règles `allow` passent, tout le reste est refusé (pour l'automatisation) |
| `auto` | `auto` | Lecture et modifications du projet sans demande ; commandes, outils MCP, chemins protégés et écritures hors du projet jugés par un classificateur (voir « Mode auto ») |
| `danger-full-access` | `bypassPermissions` | Tout est autorisé sans demande |

En `workspace-write`, l'écriture est libre dans le dossier du projet, le dossier temporaire et
les dossiers ajoutés : `permissions.additionalDirectories` dans la config, `claw --add-dir
<dossier>` (répétable) ou `/add-dir <dossier>` pendant la session (`--save` l'enregistre dans
`.claw/settings.local.json` ; `/add-dir` seul les liste). Ailleurs, Claw demande ; sans
terminal, l'écriture est refusée avec la marche à suivre.

Les commandes en lecture seule (`git status`, `git diff`, `ls`, `cat`…) ne déclenchent pas de demande.
Dans le terminal, `Maj+Tab` passe d'un mode à l'autre (demander → éditions acceptées → plan →
auto) ; le
mode actif s'affiche sous la zone de saisie. Chaque demande s'ouvre dans un encadré (commande,
diff numéroté de la modification…) avec un choix aux flèches ou au chiffre : `1` oui, `2` oui et
ne plus demander (la règle proposée est enregistrée dans `.claw/settings.local.json`), `3` ou
`Échap` non : le tour s'arrête et vous dites à Claw quoi faire à la place. Sans terminal
interactif (ou avec `CLAW_PLAIN_PROMPTS=1`), la question est posée en texte : `o`, `t`, `n` ou
une consigne libre transmise à l'agent.

Règles dans n'importe quel fichier de config (elles s'additionnent ; `deny` prime toujours) :

```json
{
  "permissions": {
    "defaultMode": "default",
    "allow": ["Bash(cargo test:*)", "Bash(npm run lint)", "WebFetch(domain:docs.rs)"],
    "deny": ["Bash(git push:*)", "Read(.env)", "Edit(secrets/**)"],
    "ask": ["Bash(git commit:*)"],
    "additionalDirectories": ["../bibliotheque-partagee"]
  }
}
```

Une commande composée (`a && b`, `a | b`, `a; b`) n'est autorisée par une règle que si chacune
de ses parties l'est ; une substitution (`$(…)`, backticks) déclenche toujours une demande.

#### Mode auto

Comme dans Claude Code, `--permission-mode auto` (ou `Maj+Tab`, ou `"defaultMode": "auto"`) laisse
Claw travailler sans demandes de routine : un second modèle, le classificateur, juge à votre
place chaque action qui en aurait demandé une.

- **Sans classificateur** : lectures, recherches, commandes en lecture seule et modifications des
  fichiers du projet. Les règles `deny` et `ask` passent avant tout, comme ailleurs.
- **Jugé** : commandes shell, outils MCP, écritures hors du projet et écritures dans un chemin
  protégé (`.git/`, `.claw/`, `.claude/`, `.vscode/`, `.husky/`, `.bashrc`, `.mcp.json`…). Les règles
  `allow` qui accordent l'exécution de code arbitraire (`Bash(*)`, `Bash(python:*)`, `npm run`…)
  sont suspendues ; les règles précises (`Bash(cargo test:*)`) restent valables.
- **Ce que voit le classificateur** : l'action, vos derniers messages (vos limites, comme « ne pousse
  pas », et vos accords explicites), `CLAW.md`/`CLAUDE.md`, le dossier et ses remotes git, et
  des règles : blocages absolus (`hard_deny` : fuite de secrets, suppression d'un chemin
  critique), blocages levables (`soft_deny` : `curl | bash`, push forcé, `git reset --hard`,
  `git clean`, déploiement en production, destruction d'infrastructure, changement de remote,
  drapeaux qui désarment une sécurité…), exceptions (`allow` : build et tests du projet,
  dépendances déclarées, commits et push vers les remotes du dépôt…). Une demande générale
  (« fais le ménage ») ne lève pas un blocage ; une demande précise (« fais un `git reset
  --hard` ») le lève.
- **Blocage** : l'action ne s'exécute pas, Claw voit la raison (`[Discarding uncommitted work]…`)
  et continue autrement. `/permissions` liste les refus récents. Après 3 blocages de suite ou
  20 au total, le mode auto se met en pause et Claw vous demande ; une approbation le relance.
  Une réponse illisible du classificateur vaut refus.
- **Réglages** (personnels seulement, `~/.claw/settings.json` ou `~/.claude/settings.json` : un
  dépôt ne peut pas élargir ce qu'accepte le classificateur) :

  ```json
  {
    "autoMode": {
      "environment": ["$defaults", "Dépôts de confiance : github.com/mon-equipe/*"],
      "allow": ["$defaults", "Déployer sur l'espace de staging est permis"],
      "soft_deny": ["$defaults", "Ne jamais lancer de migration en dehors de l'outil prévu"],
      "hard_deny": ["$defaults"],
      "classifyAllShell": false,
      "model": "openrouter/modele-classificateur"
    }
  }
  ```

  `"$defaults"` garde les règles intégrées à cet endroit ; une liste sans `"$defaults"` les
  remplace. `classifyAllShell` soumet toutes les commandes au classificateur, même celles d'une
  règle `allow`. Le classificateur utilise `autoMode.model`, sinon `smallModel`, sinon le modèle
  de la session. `"permissions": {"disableAutoMode": "disable"}` retire le mode auto.
  `claw auto-mode defaults` affiche les règles intégrées, `claw auto-mode config` les règles
  effectives.
- Le mode auto vaut aussi pour les sous-agents, les sessions en arrière-plan et l'interface web.
  Il réduit les demandes sans garantir la sûreté : gardez une règle `deny` ou `ask` pour ce
  qui doit toujours vous être soumis.

### Agent spécialisé code & architecture

Le prompt système oriente l'agent vers l'ingénierie logicielle : compréhension du code avant
d'agir, conventions du projet, **boucle de vérification** (build/tests après chaque changement,
correction jusqu'au vert, jamais de test affaibli pour passer) et méthode d'architecture
(options et compromis, recommandation, diagrammes Mermaid, ADR).

| Commande | Rôle |
|---|---|
| `/review [cible]` | Revue de code (diff courant par défaut) : bugs, sécurité, conception, par gravité |
| `/audit [portée]` | Audit du projet : carte des modules, dette technique, risques, plan d'action |
| `/archi <sujet>` | Conception : 2-3 options, recommandation, diagrammes C4/séquence, feuille de route |
| `/adr <décision>` | Rédige un ADR numéroté dans `docs/adr/` |
| `/fix [commande]` | Lance les tests et corrige jusqu'à ce qu'ils passent |
| `/avis <question>` | Second avis : la question est posée à plusieurs autres modèles, l'agent compare et tranche |
| `/security-review [cible]` | Revue de sécurité des changements : failles exploitables seulement (injection, autorisations, secrets, désérialisation, XSS, SSRF, workflows GitHub), chaque piste remontée de la source à l'usage dangereux puis mise à l'épreuve ; seules les failles jugées sûres à 8/10 au moins sont rapportées |
| `/code-review [cible\|n° de PR]` | Revue multi-agents, l'équivalent local et gratuit de la revue multi-agents de Claude Code : quatre relecteurs en parallèle (bugs, sécurité, erreurs et performances, tests et maintenabilité), puis chaque problème est vérifié dans le code et classé confirmé, plausible ou rejeté. `/code-review 42` relit la PR n° 42 (`gh pr diff`) |
| `/pr-comments [n°]` | Commentaires d'une pull request (avec `gh`), regroupés par fichier et par fil, avec une correction proposée pour chaque point encore ouvert |

**Garde-fou de sécurité à l'écriture** (l'équivalent du plugin `security-guidance` de Claude
Code, inclus d'office) : quand l'agent écrit ou modifie un fichier avec un motif risqué, le
résultat de l'écriture porte une remarque qui lui demande de corriger ou de justifier. Motifs
suivis : expression `${{ github.event… }}` dans un workflow GitHub, `exec`/`execSync` avec une
commande composée, `new Function`, `eval`, `dangerouslySetInnerHTML`, `document.write`,
`innerHTML`, `pickle.load`, `os.system`, `subprocess(..., shell=True)`, `yaml.load` sans
`SafeLoader`, requête SQL composée par concaténation ou f-string, vérification TLS coupée, secret
en clair (clés AWS, GitHub, OpenRouter, Anthropic, Slack, clés privées). Une règle n'est signalée
qu'une fois par fichier et par session ; l'écriture a lieu dans tous les cas.
`"securityGuidance": false` dans les réglages ou `CLAW_SECURITY_GUIDANCE=0` le désactivent.

L'outil `Agent` délègue à des sous-agents **synchrones** qui renvoient leur rapport :
`architect`, `reviewer`, `auditor` (lecture seule), `tester` (corrige), `Explore`, `Plan`,
`general-purpose`. Ils héritent du modèle et des permissions de la session ; leurs demandes
d'approbation passent par la même invite. Modèle par rôle (identifiants exacts sur
openrouter.ai/models) :

```json
{
  "agents": {
    "architect": { "model": "anthropic/claude-sonnet-4.5" },
    "reviewer":  { "model": "openai/gpt-5" },
    "tester":    { "model": "qwen/qwen3-coder" }
  }
}
```

Agents personnalisés au format de Claude Code dans `.claw/agents/<nom>.md` (ou `~/.claw/agents/`) :

```markdown
---
name: security-reviewer
description: Cherche les vulnérabilités OWASP
tools: Read, Grep, Glob, Bash
model: openai/gpt-5
---
Tu analyses le code à la recherche d'injections, de secrets exposés…
```

### Serveurs MCP

Déclarez vos serveurs dans `mcpServers` (`.claw.json`, `.claw/settings.json` ou
`~/.claw/settings.json`). Ils sont connectés au démarrage ; leurs outils apparaissent sous la
forme `mcp__<serveur>__<outil>` et sont aussi disponibles pour les sous-agents.

```json
{
  "mcpServers": {
    "fichiers": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
    "distant": {
      "type": "http",
      "url": "https://exemple.com/mcp",
      "headers": { "Authorization": "Bearer ${MON_TOKEN}" }
    }
  }
}
```

- Transports : `stdio` (processus local ; sous Windows, `npx`/`uvx` passent par `cmd /C`
  automatiquement), `http` (Streamable HTTP, session `Mcp-Session-Id`) et `sse` (ancien protocole).
- Gestion en ligne de commande : `claw mcp add <nom> -- <commande> [args]` (stdio, `-e CLE=val`),
  `claw mcp add --transport http|sse <nom> <url>` (`-H "Nom: valeur"`), `claw mcp add-json`,
  `claw mcp list`, `get`, `remove` ; `--scope project|local|user`.
- **OAuth** : `claw mcp login <nom>` connecte un serveur distant protégé (découverte, enregistrement
  du client, PKCE, navigateur) ; le jeton est conservé dans `~/.claw/mcp-oauth.json` et renouvelé
  automatiquement. `CLAW_NO_BROWSER=1` affiche seulement l'adresse (machine distante).
- **Ressources** : outils `ListMcpResources` et `ReadMcpResource` quand un serveur en expose.
- **Prompts** : chaque prompt MCP devient une commande `/mcp__<serveur>__<prompt> arguments`.
- `${VAR}` dans `url` et `headers` est remplacé par la variable d'environnement (pas de jeton en clair).
- `/mcp` affiche l'état des serveurs et leurs outils. Le journal d'un serveur stdio est écrit dans
  `%TEMP%\claw-mcp\<serveur>.log`.
- Permissions : un outil annoté `readOnlyHint` s'exécute sans demande ; les autres demandent
  confirmation en mode `prompt`. Pour autoriser tout un serveur :
  `"permissions": { "allow": ["mcp__fichiers"] }`.
- `CLAW_MCP_TIMEOUT_MS` (connexion, défaut 60 s) et `CLAW_MCP_TOOL_TIMEOUT_MS` (appel, défaut 300 s).

### Sessions longues, interruption et annulation

- **Compaction automatique** : à l'approche de la fenêtre de contexte du modèle, et au plus tard
  vers 120 000 jetons même si sa fenêtre est plus grande, les anciens messages sont résumés par
  le modèle (les plus récents sont gardés tels quels). Un texte qui n'a rien d'un résumé (le
  modèle a continué la conversation au lieu de la résumer) est redemandé une fois, puis
  remplacé par le résumé de Claw. Sous chaque résumé, Claw note lui-même vos demandes mot pour
  mot (la première et les plus récentes) et les fichiers modifiés : elles survivent aux
  compactions successives, et un « continue » retrouve la demande qu'il poursuit. Seuil réglable
  avec `CLAW_AUTO_COMPACT_TOKENS`, désactivation avec `CLAW_AUTO_COMPACT=0`.
- **Contexte allégé** : la page et le terminal affichent le résultat complet de chaque outil,
  mais la session ne garde pour le modèle que ce qu'il ne sait pas déjà. Après une modification :
  le fichier et les lignes touchées, sans l'ancien ni le nouveau texte qu'il vient d'écrire ;
  après une écriture : le fichier et son nombre de lignes ; après une commande : sa sortie et son
  code de retour. Le raisonnement du modèle ne lui est renvoyé que pour le tour en cours. Dans
  une vraie session, ces redites occupaient plus de la moitié du contexte. Une session
  enregistrée avant cet allègement (ou pendant qu'il était coupé) est allégée à sa réouverture.
- **Interrompre un tour** : `Ctrl+C` dans le terminal (un second `Ctrl+C` dans les 2 s quitte),
  bouton « Arrêter » ou touche `Échap` dans l'interface web. Les commandes shell en cours sont
  arrêtées avec leurs sous-processus.
- **Annuler** : `/undo [n]` restaure les fichiers modifiés par l'agent (`write_file`, `edit_file`,
  `NotebookEdit`) pendant les n derniers tours et ramène la conversation avant eux ;
  `/checkpoints` liste les points de restauration. Les modifications faites par des commandes
  shell ne sont pas suivies.
- **Compacter à la demande** : `/compact [consigne]` fait résumer l'historique par le modèle tout
  de suite, même sous le seuil automatique ; la consigne dit ce qu'il faut garder
  (`/compact garde les erreurs de build et le plan de migration`). Les hooks `PreCompact` la
  reçoivent dans `custom_instructions`.

### Travail continu, apartés et routines

Les équivalents gratuits des commandes de Claude Code pour travailler longtemps sans surveiller :

| Commande | Effet |
|---|---|
| `/goal <condition>` | Claw travaille jusqu'à ce que la condition soit remplie : après chaque tour, un contrôle (sans outils, hors conversation) répond « atteint » ou « pas encore, parce que… », et Claw continue avec cette raison. Pause sur `Échap`, après `CLAW_GOAL_MAX_TURNS` tours (12) ou quand deux contrôles de suite donnent la même raison. `/goal` montre l'état, `/goal reprendre` relance, `/goal clear` efface. La ligne d'état affiche `◎ objectif actif`. |
| `/loop [intervalle] [demande]` | Dans le terminal : relance la demande (ou `/ma-commande`) entre les tours, la saisie restant libre : toutes les `5m`, `1h`… ; sans intervalle, Claw choisit l'attente ; seul, `loop.md` ou une passe d'entretien. `/loop list`, `/loop stop [id]`. Voir « Tâches planifiées ». Même fonctionnement dans la page web. |
| `/schedule <horaire> <demande>` | Routine qui tourne chez GitHub, ordinateur éteint : un workflow `.github/workflows/claw-routine-<nom>.yml` lance l'action Claw selon l'horaire avec un modèle gratuit d'OpenRouter, et ouvre une pull request si des fichiers changent. Horaires : `horaire`, `quotidien[@HH:MM]`, `jours-ouvres[@HH:MM]`, `hebdo[@jour-HH:MM]` (heure UTC, ou `@09:00+02:00`), ou `"M H J M S"` en cron. `/schedule` liste, `/schedule delete <nom>` supprime. La demande est rangée à part dans `.github/claw-routines/<nom>.md`. |
| `/btw <question>` | Question en aparté : réponse courte du modèle sans outils, qui n'entre pas dans la conversation et n'interrompt pas la tâche. |
| `/recap` | Bilan de la session : demandes, travail fait, reste à faire, prochaine étape. Hors conversation. |
| `/copy [n]` | Copie la dernière réponse (ou la n-ième en partant de la fin) dans le presse-papiers (PowerShell, `pbcopy`, `wl-copy`, `xclip` ou `xsel`, sinon la séquence OSC 52 du terminal, qui marche aussi par SSH). |
| `/rename [titre]` | Titre de la session en cours ; sans titre, le modèle en propose un d'après la conversation. |
| `/output-style [nom]` | Style des réponses : `default`, `explanatory` (explique ses choix avec des encarts « ※ Insight »), `learning` (vous fait écrire les morceaux de logique qui comptent, marqués `TODO(human)`), `concise`. `/output-style new <nom>` crée un style à vous dans `.claw/output-styles/` (fichier Markdown avec `name` et `description` en en-tête) ; ceux de `~/.claw/output-styles/` valent pour tous les projets. Le choix est enregistré dans `.claw/settings.local.json` (`"outputStyle"`). |

Activer une routine créée par `/schedule` :

```bash
git add .github && git commit -m "Routine" && git push
gh secret set OPENROUTER_API_KEY          # clé gratuite : https://openrouter.ai/keys
# Settings › Actions › General : « Allow GitHub Actions to create and approve pull requests »
gh workflow run claw-routine-<nom>.yml    # essai immédiat
```

L'action utilisée est `abasse-ali/claw_code/.github/actions/claw@main` ; `CLAW_ROUTINE_ACTION` ou
la clé `"routineAction"` des réglages en choisissent une autre (fork, version figée).

**Ligne d'état personnalisée** : comme dans Claude Code, la clé `statusLine` des réglages
remplace la ligne sous la saisie par la sortie d'une commande (trois lignes au plus). La commande
reçoit sur son entrée standard un JSON avec `session_id`, `transcript_path`, `cwd`,
`model.id`, `workspace.current_dir`, `permission_mode`, `output_style.name`,
`context_window.used_percentage`, `cost.total_cost_usd` et `goal`. Une commande en échec, muette
ou plus lente que `timeout` (2 s par défaut) laisse la ligne intégrée.

```json
{ "statusLine": { "type": "command", "command": "~/.claw/statusline.sh", "timeout": 2 } }
```

```bash
#!/bin/sh
# ~/.claw/statusline.sh : modèle, dossier et contexte
input=$(cat)
model=$(printf '%s' "$input" | jq -r '.model.id')
dir=$(printf '%s' "$input" | jq -r '.workspace.current_dir' | xargs basename)
used=$(printf '%s' "$input" | jq -r '.context_window.used_percentage // 0')
printf '%s · %s · contexte %s %%\n' "$model" "$dir" "$used"
```

Avec `claw --resume <session>`, `/copy`, `/rename`, `/output-style` et `/schedule` marchent aussi ;
`/goal`, `/loop`, `/btw` et `/recap` demandent une session interactive.

### Hooks

Des commandes shell lancées à des moments clés, configurées dans `.claw/settings.json` (même
format que Claude Code, ou une simple liste de commandes) :

```json
{
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "python hooks/garde.py", "timeout": 30 }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "cargo test -q || (echo 'les tests échouent' >&2; exit 2)" }] }],
    "SessionStart": ["git log --oneline -5"]
  }
}
```

| Événement | Quand | Code de sortie 2 |
|---|---|---|
| `PreToolUse` / `PostToolUse` | avant / après un outil (`matcher` : nom d'outil, `Edit\|Write`, regex) | refuse l'appel / marque une erreur |
| `PostToolUseFailure` | après un outil en échec (en plus de `PostToolUse`), avec `error` | marque une erreur |
| `PermissionRequest` | avant de demander une autorisation à l'utilisateur | refuse à sa place |
| `UserPromptSubmit` | à chaque demande ; stdout ajouté au contexte | rejette la demande |
| `Stop` / `SubagentStop` | fin de tour de l'agent / d'un sous-agent | relance l'agent avec la raison (5 fois max) |
| `SubagentStart` | un sous-agent démarre (`matcher` : son type) ; stdout ajouté à sa demande | - |
| `SessionStart` / `SessionEnd` | début (`startup`, `resume`, `clear`) / fin de session | - |
| `PreCompact` | avant une compaction (`auto`, `manual`), avec la consigne de `/compact` | - |
| `Notification` | quand une autorisation est demandée (en arrière-plan) | - |

Chaque hook reçoit un JSON (`hook_event_name`, `session_id`, `cwd`, champs de l'événement) ;
une commande le lit sur stdin et a aussi `CLAW_PROJECT_DIR` (et `CLAUDE_PROJECT_DIR`), la
racine du dépôt. La raison d'un blocage est lue sur stderr (sinon stdout). Délai par défaut :
60 s. Les hooks des plugins sont aussi appliqués. `/hooks` affiche la configuration.

**Types de hooks** (champ `type`, comme dans Claude Code) :

- `command` : commande shell (par défaut).
- `http` : POST du JSON vers `url` ; `headers` accepte `${VAR}` (variable d'environnement). Une
  réponse 2xx se lit comme la sortie d'une commande (texte ou JSON) ; un autre statut avertit.
- `prompt` : la consigne (`prompt`, où `$ARGUMENTS` est remplacé par le JSON de l'événement) est
  posée à un modèle gratuit (`model`, sinon le modèle par défaut, avec les modèles de second
  avis en secours), qui répond `{"ok": true}` ou `{"ok": false, "reason": "…"}` : un garde-fou
  en langage naturel, sans script à écrire.
- `"async": true` sur n'importe quel hook : lancé sans attendre (journalisation, notifications),
  son résultat est ignoré.

```json
{
  "hooks": {
    "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "prompt",
      "prompt": "Bloque toute commande qui supprime ou déplace des fichiers. $ARGUMENTS" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "http", "url": "https://journal.exemple/claw",
      "headers": { "Authorization": "Bearer ${JOURNAL_TOKEN}" }, "async": true }] }]
  }
}
```

**Sortie JSON** (format de Claude Code) : `decision: "block"` et `reason` ;
`hookSpecificOutput.additionalContext` ; `systemMessage` ; pour `PreToolUse`,
`permissionDecision` : `deny` (refus), `allow` (autorisé sans demander, seules les règles
`deny` s'appliquent encore), `ask` (l'utilisateur confirme même si le mode l'autoriserait) et
`updatedInput` (arguments de l'outil remplacés, par exemple une commande corrigée) ; pour
`PermissionRequest`, `decision: {"behavior": "allow" | "deny", "message"}` ; sur
`PreToolUse` et `PostToolUse`, `"continue": false` (avec `stopReason`) arrête l'agent : les
appels restants de la réponse ne sont pas exécutés et le tour se termine. Les hooks
`PreToolUse` passent désormais avant le contrôle des autorisations, comme dans Claude Code.

**Règles depuis la session** : `/permissions allow <règle>`, `/permissions deny <règle>`,
`/permissions ask <règle>` ajoutent une règle à `.claw/settings.local.json` (et l'appliquent
aussitôt) ; `/permissions remove <règle>` la retire ; `/permissions rules` les liste.
Exemples de règles : `Bash(npm test:*)`, `Edit(src/**)`, `WebFetch(domain:docs.rs)`,
`mcp__serveur`.

### Commandes personnalisées

Un fichier Markdown par commande dans `.claw/commands/` (ou `.claude/commands/`,
`.codex/commands/`, et `~/.claw/commands/` pour toutes vos sessions) :

```markdown
---
description: Revue ciblée d'un fichier
argument-hint: <fichier> <axe>
---
Fais une revue du fichier $1 en te concentrant sur $2.
```

`/revue src/lib.rs "la sécurité"` envoie ce texte avec `$1`, `$2` (jusqu'à `$9`) et `$ARGUMENTS`
remplacés ; sans `$ARGUMENTS`, les arguments sont ajoutés à la fin. Un sous-dossier donne aussi
la forme `/dossier:nom`. Fonctionne dans le terminal, en ligne de commande (`claw /revue …`) et
dans l'interface web ; `/help` les liste.

En-tête facultatif : `model` (modèle utilisé pour ce tour), `allowed-tools` (outils autorisés,
ex. `Bash(git status:*), Read`). Si `allowed-tools` autorise `Bash`, chaque `` !`commande` `` du
texte est remplacée par sa sortie (ex. `` !`git diff --stat` ``) ; `@chemin` joint un fichier.

### Fournisseurs et modèles (sans dépendre d'un seul service)

- **OpenRouter** (`vendeur/modèle`, ex. `qwen/qwen3-coder:free`) et **Anthropic** (`claude-…`).
- **Serveurs compatibles OpenAI** avec un préfixe `nom:` : `ollama:qwen2.5-coder:7b`,
  `lmstudio:…`, `llamacpp:…` (locaux, sans clé, hors ligne) ; `groq:`, `mistral:`, `deepseek:`,
  `gemini:`, `cerebras:`, `nvidia:`, `together:`, `fireworks:` (clé `<NOM>_API_KEY`). Adresse modifiable par
  `<NOM>_BASE_URL` (et `OLLAMA_HOST`). Vos propres serveurs :

  ```json
  { "providers": { "gpu": { "baseUrl": "http://serveur:8000/v1", "apiKeyEnv": "GPU_KEY", "contextWindow": 65536, "toolMode": "text" } } }
  ```
- Sans aucune clé, Claw utilise le premier modèle d'un serveur local qui répond (Ollama…).
  `claw models local` (ou `/models local`) liste les serveurs et leurs modèles. Pour Ollama,
  lancez-le avec `OLLAMA_CONTEXT_LENGTH=32768` : son contexte par défaut est trop petit.
- **Le même modèle réessaie seul ; changer de modèle, c'est vous qui décidez.** Dans le terminal,
  l'interface web et le panneau de VS Code, Claw ne passe jamais à un autre modèle de lui-même.
  Quand le modèle de la session ne répond plus pour une raison passagère (panne ou surcharge du
  fournisseur, limite par minute, réseau), **il réessaie tout seul, sans rien vous demander** :
  après 3 s, 10 s, 30 s, 1 min, puis toutes les 2 min, douze essais en une vingtaine de minutes.
  Pendant ce temps, une barre au-dessus de la zone de saisie affiche le compte à rebours et
  propose, si vous le souhaitez, de réessayer tout de suite, de **changer de modèle** (vous le
  choisissez parmi les secours configurés et le gratuit le plus puissant, ou vous tapez un
  identifiant ; il devient le modèle de la session) ou d'arrêter le tour. Quand réessayer ne
  changerait rien, la barre le dit et propose d'abord de changer de modèle. La question n'attend
  votre réponse que si les essais automatiques sont épuisés, ou si réessayer ne servirait à rien
  (quota du jour épuisé, clé refusée, modèle introuvable). Quand l'agent n'avance plus (voir
  « Escalade »), la même barre propose un autre modèle ; sans réponse en 60 s, le même modèle
  continue. `"modelSwitch": "auto"` (ou `CLAW_MODEL_SWITCH=auto`) rétablit la bascule sans
  question décrite ci-dessous ; elle s'applique aussi d'office quand personne n'est là pour
  répondre (`claw prompt` dans un script, action GitHub, sous-agents). Sans personne pour
  répondre et sans autre modèle à essayer, un échec passager (modèle gratuit limité un moment
  par son fournisseur, surcharge) n'arrête plus le tour : le même modèle est réessayé après
  10 s, 30 s, 1 min, 2 min, 3 min puis 4 min, une dizaine de minutes en tout, avec une notice à
  chaque fois. Si le tour échoue quand même, la sortie `--output-format json` donne l'erreur et
  ces notices.
- **Appel d'outil coupé** : il arrive qu'un fournisseur close la réponse juste après le nom de
  l'outil, avant ses arguments. Claw ne prend pas cet appel vide pour un vrai : la réponse est
  redemandée deux fois au même modèle, puis la barre vous propose de réessayer, de changer de
  modèle ou d'arrêter. Si c'est la limite de sortie du modèle qui a coupé les arguments, rien
  n'est exécuté et le modèle est invité à envoyer un appel plus petit. De même quand la réponse
  s'arrête au milieu de longs arguments (4 000 caractères ou plus, la même requête serait
  coupée au même endroit) ou à la dernière tentative sans personne pour décider : le modèle
  apprend après combien de caractères son appel a été coupé et comment écrire un long fichier
  en plusieurs parties, au lieu de perdre le tour. Un fournisseur qui arrête la réponse sur
  `finish_reason: "error"`, sans autre message, est en panne (502) : la requête repart sur un
  autre modèle, comme après une erreur 504. Vu dans un essai réel : un `write_file` de page web
  coupé après son chemin, trois fois de suite, puis le tour abandonné. Dans tous les cas, un
  appel sans ses arguments obligatoires (ou au JSON invalide) revient au modèle avec ce qu'il
  manque, **sans vous demander d'autorisation** pour une modification vide. Les sous-agents
  ont le même contrôle.
- **Bascule automatique** : si le modèle répond « quota atteint » (429), est indisponible ou si le
  serveur est arrêté, la requête part sur le modèle suivant : `"fallbackModels": ["groq:llama-3.3-70b-versatile", "ollama:qwen2.5-coder:7b"]`
  (ou `CLAW_FALLBACK_MODELS`, liste séparée par des virgules). Sans liste, un modèle gratuit
  d'OpenRouter bascule sur les suivants du classement ; `"free"` dans la liste insère ce
  classement à sa place : `["free", "ollama:qwen2.5-coder:7b"]` essaie les meilleurs modèles
  gratuits puis le modèle local (hors ligne, quota épuisé). Limite par minute (20 requêtes pour
  les modèles gratuits d'OpenRouter) : Claw attend sa remise à zéro ; surcharge passagère d'un
  fournisseur : quelques réessais sur place ; quota du jour épuisé ou modèle refusé à Claw (403,
  « only available on agentic harnesses ») : mis de côté jusqu'à la remise à zéro ou 24 h, et
  retenu d'un lancement à l'autre (`~/.claw/model-cooldowns.json`). Un modèle introuvable chez son
  fournisseur (404, sauf le « no endpoints found » d'OpenRouter, qui dépend de la requête) est mis
  de côté six heures : vu le 20 octobre 2026, Nemotron 3 Super, introuvable chez NVIDIA, était
  redemandé à chaque étape. Un autre échec met le modèle de côté un moment ; `CLAW_FALLBACK=0`
  désactive la bascule.
  **Panne en cours de réponse** : quand le fournisseur d'un modèle lâche alors qu'il a déjà
  commencé à répondre (504 « Upstream idle timeout », surcharge, erreur interne), le tour ne
  s'arrête plus sur une erreur : la même requête repart sur le modèle suivant de la chaîne, et le
  modèle fautif est mis de côté cinq minutes pour que les étapes suivantes ne l'attendent pas
  (« … a cessé de répondre (erreur 504) → la requête repart sur … »). Sans autre modèle, une
  nouvelle tentative est faite. Les sous-agents font de même.
- **Fichiers de travail** : tout ce que Claw écrit pour lui-même dans un projet est rangé dans
  `.claw/` (sessions, points de restauration, liste de suivi `todos.json`, dossiers `sandbox/`
  des commandes isolées), rien à la racine. Les `.sandbox-home`, `.sandbox-tmp` vides et le
  `.claw-todos.json` laissés par une version précédente sont retirés ou déplacés au premier usage.
- **Serveurs stricts** : l'API de Mistral refuse tout champ absent de sa spécification. Claw ne
  lui envoie donc pas `stream_options` (elle donne l'usage des jetons sans qu'on le demande), et
  si un autre serveur refuse ce champ, Claw le retient et renvoie la requête sans lui.
- `"smallModel"` (ou `CLAW_SMALL_MODEL`) : petit modèle utilisé pour les résumés de compaction.

### Fiabilité avec les petits modèles

- **Serveurs et surveillances** : une commande qui ne rend jamais la main (`npm run dev`,
  `uvicorn`, `flask run`, `vite`, `next dev`, `--watch`) part d'office en arrière-plan au lieu de
  bloquer le tour ; le modèle la suit avec `BashOutput` et l'arrête avec `KillShell`. Le texte
  entre guillemets, un document en ligne (`<<`) et les commentaires ne comptent pas (écrire un
  README qui cite `npm run dev` ne lance rien en arrière-plan), ni un serveur que la commande
  détache elle-même pour l'essayer (`uvicorn app:app & sleep 3; curl …; kill $!`). Avec
  `run_in_background: false`, le modèle garde la main.
- **Commandes longues** : rien n'est tué par défaut. Sans délai fixé par le modèle, une commande
  encore en cours au bout de 5 minutes (une installation, une compilation) **continue en
  arrière-plan** avec sa sortie déjà reçue, et le modèle attend sa fin en une seule requête
  (`BashOutput` avec `block: true`). Il en va de même, sans attendre, pour un serveur qui
  s'annonce prêt (« Listening on… », « Running on http://… ») puis se tait, et pour un processus
  laissé par `&` qui tient encore la sortie. Un délai fixé par le modèle est respecté : la
  commande est arrêtée à l'échéance, et sa sortie déjà produite est rendue (avant : perdue). Un
  délai sous 1 000 vient d'un modèle qui pensait en secondes (`120`) : il est lu en secondes.
  La sortie est lue au fil de l'eau (Python écrit sans tampon), ce qui vaut aussi pour l'outil
  PowerShell : une grosse sortie ne le bloque plus, et ses commandes en arrière-plan se lisent
  et s'arrêtent comme celles de bash. Une commande partie en arrière-plan ne compte jamais comme
  une vérification réussie.
- **Code des erreurs** : quand une commande ou la vérification échoue en citant des emplacements
  du projet (trace Python, erreur Rust, TypeScript `fichier(ligne,colonne)`, Node, Go, pytest),
  Claw joint les lignes de code autour de chacun (trois au plus) : une relecture de moins, donc
  une requête de moins. Une trace Python est lue depuis la fin, où est le cadre qui lève
  l'erreur. Les dépendances (`node_modules`, `.venv`…) et les adresses web ne sont pas montrées.
  `CLAW_ERROR_SITES=0` le coupe.
- **Lecture** : `read_file` rend 2 000 lignes au plus quand le modèle ne précise pas `limit`, et
  dit à partir de quel `offset` lire la suite ; une ligne de plus de 2 000 caractères (code
  minifié) est raccourcie ; un fichier binaire est signalé comme tel au lieu d'une erreur
  d'encodage.
- **Codages** : un fichier en Windows-1252 (CSV d'Excel, anciens fichiers Windows ; l'euro,
  l'apostrophe typographique et « œ » compris), en UTF-16 (écrit par `>` de PowerShell 5) ou en
  UTF-8 abîmé par quelques octets se lit, se cherche (`grep_search`) et se modifie. `edit_file`
  et `write_file` **gardent son codage** : un CSV d'Excel reste lisible par Excel. Un caractère
  que ce codage ne peut pas stocker est refusé par `edit_file` avec un message qui le nomme ;
  `write_file` passe alors le fichier en UTF-8 et le dit.
- **Recherche** : `grep_search` et `glob_search` sautent les dépendances, environnements et
  caches (`node_modules`, `.git`, `.venv`, `__pycache__`, `.next`, `.claw`…), sauf si le chemin
  ou le motif les nomme. Un dossier `dist`, `build`, `target`, `out` ou `coverage` n'est sauté
  que si c'est bien une sortie de build (ignoré par un `.gitignore`, ou marqué par Cargo) : un
  paquet Go `internal/build` ou un script nommé `build` restent visibles. Avant, une recherche
  lisait toutes les dépendances (des dizaines de milliers de fichiers dans un projet Next.js) et
  `glob_search` les montrait en premier, ce qui égarait le modèle. Un dossier illisible ne fait
  plus échouer la recherche. Un fichier de plus de 4 Mo trouvé en parcourant un dossier n'est
  pas lu, mais celui que nomme `path` (un journal) l'est toujours. Quand quelque chose n'a pas
  été cherché, le résultat le dit (`note`), pour qu'un résultat vide ne passe pas pour une
  absence.
- **Sorties bornées** : la sortie d'une commande gardée dans la session est limitée à
  30 000 caractères (le début et la fin, où sont les erreurs, avec la mention de ce qui manque),
  au lieu de repartir en entier avec chaque requête.
- **Requêtes regroupées** : la consigne demande au modèle de réunir dans une même réponse les
  lectures et recherches indépendantes (chaque réponse coûte une requête du quota).
- `edit_file` tolère les différences de fins de ligne (CRLF), d'espaces de fin et d'indentation
  (le nouveau texte est réindenté), refuse une occurrence ambiguë et indique la ligne la plus
  proche quand le texte est introuvable.
- Les arguments JSON mal formés des appels d'outils sont réparés (bloc de code, virgules finales,
  retours à la ligne bruts, `True`/`None`, sortie tronquée).
- **Mode texte** pour les modèles sans appel d'outils natif : les outils sont décrits dans le
  prompt et le modèle répond en blocs `<tool_call>`. Il s'active seul quand le serveur refuse le
  paramètre `tools` (et reste retenu), ou avec `"toolMode": "text"` / `CLAW_TOOL_MODE=text`.
- Un appel écrit en JSON brut au lieu d'un appel natif (`{"name": "read_file", "arguments": …}`,
  fréquent avec Ollama) est reconnu s'il forme toute la réponse et nomme un outil proposé.
- Un appel écrit à la manière de Claude (`<invoke name="edit_file"><parameter name="path">…`),
  ou de Qwen3-Coder (`<tool_call><function=grep_search><parameter=path>…`, écrit par Nemotron 3
  Ultra dans une session de Claw Web : l'utilisateur lisait la balise au lieu de voir l'appel),
  dans une balise quelconque (`<function_calls>`, `<dots_function_call>`…), devient un vrai appel
  d'outil : dans la réponse, s'il la termine (le XML ne s'affiche pas), et dans le raisonnement
  quand la réponse n'a ni texte ni appel (un modèle de la session de test y avait écrit sa
  modification, perdue). Il doit nommer un outil proposé, être complet (un appel coupé par la
  limite de sortie n'écrirait qu'un fichier tronqué) et ne pas être un exemple dans un bloc de
  code. Un résultat que le modèle inventerait à la suite est écarté.
- Une réponse faite seulement de raisonnement, sans texte ni appel d'outil, compte comme vide :
  Claw relance le modèle une fois au lieu de terminer le tour sans rien dire.
- Les descriptions de `edit_file` et `write_file` disent quand utiliser l'un ou l'autre : une
  modification courte et exacte d'un fichier existant, ou le contenu complet d'un fichier.

### Aides au raisonnement

Un grand modèle vérifie son travail de lui-même ; un petit modèle gratuit l'oublie, s'égare ou
tourne en rond. Ces aides ne sont pas des consignes ajoutées au prompt : c'est **Claw qui les
applique**, quel que soit le modèle.

| Aide | Ce que fait Claw | Couper |
|---|---|---|
| Vérification imposée | Quand le modèle dit avoir fini après avoir modifié des fichiers, Claw lance la vérification du projet (`cargo check`, `go build`, `tsc`, tests Python… ou `"verify"` ; des tests Python écrits pour pytest, qui l'importent ou ne sont que des fonctions `test_...`, sont lancés avec pytest s'il est installé, même sans fichier de réglage ; un venv du projet, `.venv` ou `venv`, fournit son Python : vu dans une session de Claw Web, la vérification lancée avec le Python du système échouait, et l'agent installait les dépendances dans le système pour la faire passer). En cas d'échec, la sortie repart au modèle, qui corrige (3 corrections au plus) ; si cela échoue encore, Claw l'annonce : « le travail n'est pas validé » | `CLAW_VERIFY=0` |
| Syntaxe immédiate | Après chaque modification, contrôle de syntaxe du fichier (JSON, Python, JavaScript, Rust, Go, shell, l'imbrication des balises HTML : un élément fermé par la balise d'un parent, une balise de fin orpheline, avec leurs lignes ; et, avec leur propre analyseur, PowerShell, TypeScript, CSS et TOML) ; l'erreur est jointe au résultat de l'outil. Un script PowerShell est lu par l'analyseur de Windows PowerShell 5.1, celui de tout Windows : la syntaxe de PowerShell 7 (`&&`, `??`, `?:`) y est une erreur, comme chez l'utilisateur, sauf si le script demande la version 7 (`#Requires -Version 7`). Un fichier TypeScript est débarrassé de ses types par Node avant d'être lu (`node --check` seul le lit comme du JavaScript et se trompe). Vu dans un essai réel : deux `<article>` restés ouverts défaisaient les cartes d'une page sans que rien ne le signale | `CLAW_DIAGNOSTICS=0` |
| Pièges du code | Un fichier de code qui vient d'être écrit est relu pour les pièges qui font un code faux sans erreur de syntaxe, chacun avec sa ligne et sa correction, dit une fois par tour. **Python** (par l'interpréteur lui-même, `ast` et `symtable`) : un nom jamais défini, importé ni intégré (une faute de frappe, un import oublié), un `f` oublié devant une chaîne qui montre `{total}` telle quelle, un argument par défaut mutable, `is` devant un littéral, un `except:` nu ou un `except Exception: pass`, `open()` ou `read_text()` sans `encoding=` (le code de page de Windows abîme les accents), `__name__ == "main"`. **PowerShell** : `>` ou `<` dans une condition (ils redirigent vers un fichier), `$null` à droite d'une comparaison (un tableau à gauche est filtré), `=` dans une condition, un paramètre de PowerShell 7 dans un script pour 5.1, `Out-File` ou `Set-Content` sans `-Encoding` (UTF-16 ou Windows-1252 en 5.1), un `try` autour de commandes de fichiers ou de services dont le `catch` ne voit jamais les échecs. **Batch** : une étiquette introuvable, `set x = y` (le nom prend l'espace), `%var%` lu dans le bloc `( )` qui la change, un `)` dans un `echo` de bloc, `if %X%==` sans guillemets, `exit` sans `/b`, des accents sans `chcp 65001`. **Shell** : shellcheck s'il est installé. **PowerShell lancé depuis bash** : une commande `powershell -Command "…"` entre guillemets doubles avec des `$` voit ses variables remplacées par bash avant que PowerShell ne la lise (vu dans un essai réel : `$_.FullName` devenu `14377.FullName`, quatre essais perdus) ; Claw le dit au modèle, avec la correction (guillemets simples, ou `-File`). Essayé sur des scripts écrits par des professionnels (`gradlew.bat`, `Activate.ps1`, l'intégration de terminal de VS Code, l'installateur de Claw) et 40 fichiers Python : un seul constat, juste | `CLAW_PITFALLS=0` |
| Conventions des scripts | Ce que l'interpréteur exige est imposé à l'écriture, à la création comme à chaque modification : un `.bat` ou un `.cmd` en fins de ligne CRLF et sans BOM (en LF, `cmd.exe` perd ses étiquettes et `goto` échoue ; un BOM casse la première commande), un script shell en LF (bash échoue sur CRLF : `$'\r': command not found`), un script PowerShell accentué en UTF-8 avec BOM (Windows PowerShell 5.1 lit sinon le fichier en Windows-1252). Le résultat de l'outil le dit au modèle | (toujours) |
| Méthode de code | Votre demande écrit ou corrige du code et nomme son langage (« écris un script PowerShell », « corrige le script Python ») : Claw joint la méthode d'un ingénieur soigneux, commune à tout code (dire ce qui entre, ce qui sort et les cas qui cassent un code naïf : entrée vide ou absente, chemin avec espaces ou accents, fichier verrouillé, deuxième lancement ; ne jamais inventer une API ; une erreur jamais avalée ; aucun secret ni chemin personnel dans le code ; finir en le lançant), puis les règles du langage : Python, PowerShell (viser 5.1, `[CmdletBinding(SupportsShouldProcess)]`, `Set-StrictMode`, `$ErrorActionPreference = 'Stop'`, `-WhatIf` sur tout changement, `-LiteralPath`, `$LASTEXITCODE`), batch (`setlocal EnableDelayedExpansion`, tout entre guillemets, `!var!` dans les blocs, `if errorlevel 1`, `exit /b`), Bash (`set -euo pipefail`, guillemets, `mktemp` et `trap`), JavaScript et TypeScript (promesses toutes attendues, `process.exitCode`, mode strict sans `any`), et plus court Rust, Go, C#, Java, PHP et SQL. Le premier fichier écrit dans un langage pendant le tour reçoit aussi ses règles et la commande pour l'essayer | `CLAW_CODE_GUIDE=0` |
| Script jamais lancé | Le modèle a écrit dans le tour un script (un `.ps1`, un `.bat`, un script shell, un fichier Python avec `__main__` ou qui lit ses arguments, un fichier JavaScript avec `process.argv`) et ne l'a lancé par aucune commande : avant de le croire fini, Claw le renvoie l'essayer sur une petite entrée sûre (`--help`, `-WhatIf`, une copie dans un dossier temporaire), lire la sortie et corriger, ou dire dans sa réponse pourquoi il ne le lance pas (supprimer de vraies données, envoyer des messages, installer). Une fois par tour, pas en lecture seule | `CLAW_RUN_SCRIPTS=0` |
| Détection de blocage | Trois appels identiques sans effet, deux modifications ratées du même fichier, quatre échecs d'affilée, une vérification relancée alors que rien n'a changé, 40 appels dans un tour : Claw recadre le modèle | `CLAW_STALL_GUARD=0` |
| Test réécrit | Un test (ou le script de vérification) modifié après un échec est signalé à l'utilisateur et au modèle : faire passer un contrôle en le réécrivant ne prouve rien. Seules comptent les lignes qui existaient au début du tour : corriger un test que le modèle vient d'écrire n'est pas signalé | `CLAW_TEST_GUARD=0` |
| Carte du projet | Fichiers et principaux symboles du projet, donnés au début de la session (6 000 caractères, `CLAW_REPO_MAP_CHARS`) | `CLAW_REPO_MAP=0` |
| Chemins | Un chemin inventé (`/workspace/src/a.py`, `/c/…`, `/mnt/c/…`) est retrouvé s'il existe dans le projet ; sinon l'erreur donne le dossier de travail et les fichiers au nom proche | `CLAW_PATH_HELP=0` |
| Outils utiles seulement | Les outils rarement utiles ne sont proposés que lorsqu'ils servent (`BashOutput` s'il y a une tâche de fond, `NotebookEdit` s'il y a un notebook…) | `CLAW_TOOL_DIET=0` |
| Raisonnement conservé | Le raisonnement du modèle est renvoyé à l'étape suivante du même tour (OpenRouter), au lieu d'être perdu entre deux appels d'outils | `CLAW_REASONING_PASSBACK=0` |
| Relecture | Avant de rendre la main, les modifications d'au moins 5 lignes sont relues par **un autre modèle**, avec la réponse que l'agent s'apprête à vous donner : le relecteur traduit d'abord chaque phrase de votre demande en comportement observable, puis cherche où le code fait autre chose, et signale aussi ce que la réponse affirme sans que les modifications le montrent. Après un message court (« reprendre », « continue »), il reçoit aussi la demande que ce message poursuit ; quand la demande nomme un genre de fonctionnalité, il reçoit aussi ses cas limites (voir « Cas limites »), à confronter au code. Ses remarques repartent au modèle, une fois par tour. Un relecteur qui approuve sans l'analyse demandée (« LGTM » seul) ne prouve pas qu'il a relu : un second relecteur, un autre modèle, prend sa place ; s'il n'analyse pas non plus, Claw vous dit que le travail n'a pas vraiment été relu. Les modèles gratuits raisonnent longtemps sur un vrai diff : la relecture peut prendre jusqu'à 4 minutes par relecteur et 6 en tout ; un relecteur dont la réponse est partie tout entière en raisonnement est consulté une seconde fois, et un échec dit ce qui est arrivé (« réponse sans texte après 27 664 caractères de raisonnement »). Si aucun autre modèle ne répond (relecteurs gratuits saturés), le modèle de la session relit lui-même, dans une conversation neuve, plutôt que de laisser le travail sans relecture. Les phrases de la demande qui laissent une action à une personne (« lui seul peut alors l'emprunter ») sont citées au relecteur, qui dit à l'étape 1 qui fait cette action et quand : un relecteur qui y lisait un prêt d'office l'a lue juste dès que la phrase lui a été citée. `CLAW_REVIEW_LOG=fichier` garde chaque réponse du relecteur, approbation comprise | `CLAW_REVIEW=0` ou `"review": false` |
| Conseil | Au deuxième échec de la vérification, ou au deuxième recadrage, un autre modèle lit la fin de la session et donne son diagnostic | `CLAW_COUNCIL=0` |
| Second avis | L'outil `Consult` (et `/avis <question>`) pose la même question à plusieurs autres modèles ; l'agent compare et décide | `CLAW_COUNCIL=0` |
| Leçons | L'outil `Remember` note une leçon utile au projet dans `.claw/lessons.md` (80 au plus), relue au début de chaque session | `CLAW_LESSONS=0` |
| Modèle de planification | En mode plan, `"plannerModel"` réfléchit à la place du modèle de la session, qui exécute ensuite le plan approuvé | `CLAW_PLANNER=0` |
| Escalade | Au deuxième échec de la vérification, ou au deuxième recadrage, Claw vous propose de confier la suite à un autre modèle (le gratuit le plus puissant en premier) ; sans réponse en 60 s, le même modèle continue. Avec `"modelSwitch": "auto"` ou sans personne pour répondre, **le modèle gratuit le plus puissant du moment** reprend la fin du tour ; le tour suivant repart avec le modèle habituel. Sans réglage à faire, et jamais vers un modèle payant. De même quand le modèle ne répond rien (ni texte ni appel) deux fois dans un tour : un autre modèle reprend au lieu de perdre le tour | `CLAW_ESCALATION=0` |
| Erreurs de l'éditeur | Avec l'extension VS Code : quand le tour a modifié des fichiers, Claw demande à l'éditeur les erreurs qu'il y voit (compilation, types, lint) et les renvoie au modèle, deux fois au plus par tour. L'outil `EditorDiagnostics` permet aussi au modèle de les lire quand il veut | `CLAW_EDITOR_DIAGNOSTICS=0` |
| Contrôle des exigences | Quand le tour a modifié au moins 5 lignes et que la vérification passe, le modèle liste chaque partie de la demande (chaque page, fonction, fichier ou explication demandée ; après « reprendre », la demande poursuivie) et chaque exigence de la documentation du code (cas limites compris), puis donne pour chacune sa preuve : le code qui la couvre et le test ou la commande qui montre qu'elle marche. Ce qui manque est fait tout de suite, pas annoncé ni laissé à l'utilisateur. Sa réponse finale ne peut affirmer que ce que ce contrôle a établi, et nomme ce qui n'est pas fait ou pas vérifié. Il n'invente ni tests ni résultats. Pour les genres de fonctionnalité que nomme la demande (voir « Cas limites »), Claw rappelle aussi ce qu'un ingénieur expérimenté vérifie d'office : pour une recherche, les accents et les majuscules, les espaces, la recherche vide, plusieurs mots, le `+` d'une URL. Les phrases de la demande qui laissent une action à une personne y sont citées telles quelles, avec ce qu'il faut vérifier : rien n'est fait à sa place, les autres sont refusés. Une requête de plus par tour concerné | `CLAW_CHECKLIST=0` |
| Contexte allégé | Les résultats d'outils gardés dans la session ne répètent pas au modèle ce qu'il vient d'écrire (ancien et nouveau texte d'une modification, contenu d'un fichier écrit, état du bac à sable d'une commande, liste de tâches qu'il vient d'envoyer : il en reste le nombre de tâches faites et celle en cours) | `CLAW_LEAN_RESULTS=0` |
| Persévérance | Le modèle veut s'arrêter juste après au moins deux échecs d'outils d'affilée (commande en erreur, modification ratée) : Claw le renvoie une fois chercher une autre voie (corriger la commande, un autre outil, une étape plus petite) au lieu de vous dire « lancez-le dans votre terminal ». Ce n'est qu'ensuite qu'il explique ce qui bloque et ce que vous devez faire. Un appel que vous avez refusé n'est jamais compté : l'agent n'insiste pas | `CLAW_PERSISTENCE=0` |
| Signalement d'un problème | Vous dites que quelque chose ne va pas (« ça ne marche pas », « c'est n'importe quoi », une capture d'écran avec quelques mots, une erreur collée) : Claw joint à votre message, pour le modèle seulement, une méthode de débogage. Lister chaque symptôme du message et de chaque image, le reproduire comme vous l'obtenez, trouver la cause par des observations et non par une supposition, corriger, puis constater que le symptôme a disparu. Avant que la réponse parte, Claw demande au modèle ce qu'il a observé de la cause et de sa disparition : sans observation, il doit la faire ou dire que ce n'est pas vérifié, au lieu d'écrire « résolu ». Une requête de plus par signalement | `CLAW_PROBLEM_GUIDE=0` |
| Commandes essayées | La réponse vous dit de lancer des commandes (blocs de code d'un terminal) que le modèle n'a lancées à aucun moment de la session : Claw le renvoie une fois les essayer (un serveur part en arrière-plan, il l'interroge), corriger sa réponse d'après ce qu'il voit, ou dire lesquelles ne sont pas testées. Les exemples à compléter, `git`, `docker`, `curl` et les installations du système ne sont pas demandés | `CLAW_TRIED_COMMANDS=0` |
| Titres sans contenu | La réponse, sans appel d'outil, s'arrête sur des titres sans rien dessous (« ### 2. Frontend », « #### 2a. Composant URLInput »...) : le travail annoncé n'existe pas. Claw renvoie le modèle le faire, un appel d'outil par fichier, deux fois au plus par tour. Les blocs de code sont ignorés (un commentaire `#` n'est pas un titre) | `CLAW_EMPTY_TITLES=0` |
| Contenu abrégé | Un fichier réécrit avec « ... rest of the code unchanged ... » à la place de ses lignes est refusé quand il fondrait de plus de moitié (sinon il est écrit, avec une note au modèle) ; une modification qui remplace des lignes par une telle mention est refusée. Le modèle apprend quoi envoyer à la place | `CLAW_PLACEHOLDER_GUARD=0` |
| Réponse coupée | Une réponse en texte arrêtée par la limite de sortie du modèle au milieu d'une phrase : Claw demande la suite, deux fois au plus pour une même réponse, et la joint à la réponse au lieu de vous rendre un texte tronqué. Une ligne vous le signale | `CLAW_CUT_REPLIES=0` |
| Données réécrites | Des lignes existantes d'un fichier de données (`.csv`, `.tsv`, `.jsonl`...) sont remplacées, par une modification ou par une commande (`sed -i`, un script, `cat > fichier` : Claw compare alors le fichier à son état du début du tour) : Claw rappelle au modèle qu'un programme qui ne sait pas lire des données ne prouve pas qu'elles sont fausses. Si les valeurs sont valides dans leur format (virgule décimale, séparateur de milliers, autre format de date), il doit restaurer le fichier et corriger le code qui le lit ; sinon dire exactement quelles lignes il a changées et pourquoi. Vous êtes prévenu. Vu dans un essai réel : un petit modèle réécrivait de mémoire le CSV de l'utilisateur au lieu de corriger le script. Les fichiers JSON où l'application garde ses données (pas `package.json`, `tsconfig.json`, `.vscode/` ni les autres fichiers de configuration connus) sont gardés aussi : un champ ajouté à chaque entrée pour coller au nouveau code (une migration faite à la main) ou une valeur changée est signalé, car les copies de l'utilisateur gardent l'ancienne forme ; le code doit la lire aussi. Vu deux fois dans un essai réel : `"inscrits": []` ajouté à chaque atelier, puis un code qui plantait sur le fichier d'origine | `CLAW_DATA_GUARD=0` |
| Script muet | Une commande lance un script Python qui n'écrit rien, et ce script définit `main()` sans jamais l'appeler (son bloc `if __name__ == "__main__":` a disparu) : Claw le dit au modèle, avec le bloc à remettre. Vu dans un essai réel : le bloc perdu dans une réécriture, la commande de l'utilisateur ne faisait plus rien alors que les tests, qui appellent `main()` directement, passaient ; le modèle contournait en appelant `main()` lui-même | `CLAW_ENTRY_POINT=0` |
| Mode projet | Pour créer un projet complet. Une demande de création dans un dossier sans code (ou « mode projet » écrit dans la demande) ouvre le mode pour ce dossier, d'une session à l'autre (`.claw/project.json` ; « sans mode projet » l'arrête). Claw guide le modèle étape par étape : cahier des charges `SPEC.md` (une ligne `R1` par exigence observable, une ligne `E1` par cas limite, chacune avec son essai), plan `PLAN.md` en jalons (`## M1`, `Requirements:`, `Verify:`, `Status:`, puis la démonstration finale `## Demo`), choix notés dans `DECISIONS.md`, puis un jalon à la fois, ses tests d'acceptation d'abord. Claw relit ces fichiers après chaque écriture et dit ce qui l'empêche de les suivre ; il lance la commande `Verify` de chaque jalon marqué fait et renvoie un échec au modèle ; il signale une exigence d'un jalon fait qu'aucun test ne nomme ; il joint les lignes du cahier des charges au contrôle des exigences et à la relecture. Quand quelqu'un peut répondre, le cahier des charges puis chaque jalon terminé sont soumis à l'utilisateur ; sinon le modèle va au bout du plan. Au premier tour de chaque session, Claw sonde la machine (Python, pytest, Node, npm, Git, Cargo, Go, Java ; codage de la sortie de Python ; modules retirés de Python ; fuseaux horaires) et le dit au modèle avec les pièges connus. `DECISIONS.md` est rappelé à chaque tour, et réclamé une fois le plan entamé. Dès que `SPEC.md` et `PLAN.md` sont écrits et lisibles, un autre modèle les relit avant le code (relecture d'architecture, de nouveau seulement si des exigences ou des jalons changent). Quand tous les jalons sont faits, Claw copie le projet dans un dossier vide (les fichiers que Git suit ou suivrait) et y lance les lignes `Install` puis `Run` de la démonstration, comme le fera l'utilisateur : un échec repart au modèle (trois fois au plus), une réussite est notée, et le README doit montrer les mêmes commandes. Au début de chaque tour, si le quota gratuit du jour ne suffit pas pour les jalons restants (coût d'un jalon mesuré sur le projet, 30 requêtes à défaut), le modèle et l'utilisateur le savent, et le modèle s'arrête sur un état propre | `CLAW_PROJECT_MODE=0` |
| Petit projet | Au premier tour d'une session, si le dépôt Git du projet est petit (20 fichiers au plus, 24 000 caractères en tout, chaque fichier de la taille d'une vue), Claw joint tous ses fichiers au message, en vues comme après une modification : le modèle n'a pas à les lire un par un (un petit modèle y passait sept requêtes avant d'écrire quoi que ce soit). Chaque vue est remplacée quand son fichier change ; lire un fichier qu'une vue à jour montre déjà est signalé au modèle | `CLAW_SMALL_PROJECT=0` |
| Modifications perdues | Une commande du modèle supprime un fichier qu'il avait modifié pendant le tour, ou le ramène à son état du début du tour (`git checkout`, `git restore`, `git stash`) : Claw garde sa dernière version (dans `.claw/kept/`), le dit au modèle avec la commande qui la remet en place, et vous prévient. Il ne la remet pas lui-même : une suppression peut être voulue. Un fichier créé puis effacé dans le même tour (un script d'essai) ne compte pas. Vu dans un essai réel : le petit modèle a supprimé `conges.py` par une commande, l'a restauré avec `git checkout` et a refait tout son travail, deux fois | `CLAW_LOST_EDITS=0` |
| Fichier montré après modification | Le résultat d'une modification porte le fichier tel qu'il est maintenant (en entier s'il est petit, sinon les lignes autour du changement) : le modèle n'a plus à le relire avant la modification suivante, ce qui lui coûtait une requête à chaque fois, et la conversation ne garde qu'une vue à jour par fichier | `CLAW_FILE_VIEW=0` |
| Essais regroupés | Trois réponses de suite faites d'une seule lecture ou d'une seule commande (même refusée par le programme essayé) : Claw demande de regrouper ce qui est déjà prévu (plusieurs appels dans une réponse, commandes enchaînées), chaque réponse coûtant une requête du quota, et montre les trois dernières commandes réunies en un seul appel. Redit à la sixième et à la neuvième réponse isolée, trois fois au plus par tour. De même quand une réponse ne fait que mettre à jour la liste de tâches alors que du travail suit : la mise à jour part avec l'appel suivant | `CLAW_BATCH_CALLS=0` |
| Compte rendu gardé | Après un contrôle de Claw qui n'a rien fait changer, le modèle répond parfois en quelques lignes (« la relecture confirme... ») au lieu de redonner son compte rendu. Quand cette réponse fait moins du tiers de la précédente, Claw remet le compte rendu devant elle : le dernier message, le seul que vous lisez, reste complet, et la page rouvre la réponse qu'elle avait repliée | `CLAW_KEPT_ANSWER=0` |
| Compte rendu réglé | Un petit modèle écrit son compte rendu final, puis appelle encore un outil de rangement (`Remember`, `TodoWrite`, `ExitPlanMode` hors mode plan), et recommence. Claw fait ces appels sans revenir au modèle : le compte rendu est la réponse, avec les contrôles de fin habituels | `CLAW_SETTLED_ANSWER=0` |
| Relecture inutile | Le modèle relit un fichier qui n'a pas changé depuis sa lecture précédente (souvent juste avant de le modifier, par habitude) : la conversation ne garde que la copie la plus récente, et quand les deux lectures sont du même tour, Claw lui dit que celle-ci ne lui a rien appris pour le prix d'une requête, deux fois au plus par tour. Vu dans un essai réel : quatre requêtes sur les dix premières. Un fichier relu en entier après avoir changé remplace aussi sa lecture précédente (même passage ou fichier entier) : seule la version à jour reste dans la conversation. Un fichier long (plus de 12 000 caractères) lu en entier pour la troisième fois vaut au modèle une remarque : lire seulement les lignes utiles (`offset`, `limit`) ou chercher avec `grep_search`. Vu dans une vraie session de Claw Web : un gabarit de 52 Ko relu 28 fois en un tour, et 62 000 jetons envoyés à chaque requête | `CLAW_REPEATED_READS=0` |
| Faux message de l'utilisateur | Une réponse qui s'ouvre sur une marque de tour (`user`, `<\|im_start\|>user`...) ou qui contient une ligne commençant comme une note de Claw (`[Claw]`) joue le rôle de l'utilisateur : elle n'est ni montrée ni exécutée. Claw la redemande une fois au même modèle, puis au suivant de la chaîne de secours. Vu le 20 octobre 2026 dans une session de Claw Web : Step 5 Preview a écrit « user », recopié la demande et une fausse note « [Claw] Diagnostic de l'agent précédent », puis suivi les tâches qu'il y inventait (une refonte complète du site que personne n'avait demandée) | toujours actif |
| Cas limites | Quand votre demande nomme un genre de fonctionnalité (recherche ou filtre, montants, dates, CSV ou Excel, pagination, formulaires, envois de fichiers, comptes, tris, soldes et réservations, doublons, adresses e-mail et téléphones, bases SQL, clés d'API, OCR et traduction, images, appels à un service extérieur, liaison entre front et back, API JSON, code d'une page web, données gardées en JSON, fichiers texte sous Windows, scripts en ligne de commande), Claw joint pour le modèle, dès le départ, ce qu'un ingénieur expérimenté vérifie d'office pour ce genre-là (pour une recherche : accents et majuscules, « oeufs » pour « Œufs », recherche vide, plusieurs mots, le `+` d'une URL ; pour des dates : « 03/04 » est le 3 avril, une fin avant le début est refusée, le dernier jour compte), puis le rappelle, chaque point à prouver, au contrôle des exigences ou à la demande de preuves d'un problème signalé, et le relecteur le confronte au code. Chaque rappel nomme le piège précis (par exemple : `replace(" ", "")` ne retire pas l'espace insécable qu'écrit Excel) : un rappel général ne changeait rien. Les faits cités ont été vérifiés sur une vraie machine (Python 3.13 sous Windows, Pillow, OpenCV, Node), et les mots qui reconnaissent un genre évitent les faux amis (« sortie », « format », « Excellent », « erreur de connexion », les « horaires » d'ouverture d'un site vitrine). Chaque essai porte le résultat attendu : sans lui, un modèle gratuit faisait les essais qui confirmaient son code. Et une ligne qui s'applique fait partie de la demande, même si vous ne l'avez pas écrite : sans cette phrase, un modèle a lu la ligne des doublons puis l'a écartée parce que la demande « ne le dit pas explicitement ». Mesuré sur des essais réels : 9/12 devient 12/12 (recherche), 3/4 devient 4/4 (CSV), 10/12 devient 12/12 (dates et soldes), 11/12 sans l'aide devient 12/12 (doublons) | `CLAW_EDGE_CASES=0` |
| Méthode de design | Votre demande construit ou refait une interface web (« crée un site vitrine », « rends la page plus belle », « build a landing page » ; aussi un message court qui poursuit une telle demande, comme « plus coloré ») : Claw joint pour le modèle la méthode d'un studio exigeant, avec des cibles chiffrées plutôt que des conseils. D'abord un brief (sujet, public, rôle de la page, vrai contenu dans votre langue), puis un plan écrit avant le code : 4 à 6 couleurs nommées tirées du monde du sujet (l'accent sur un dixième de la page au plus, chaque texte vérifié à 4,5:1), une police de titre et une police de texte choisies pour le sujet (jamais Inter, Roboto, Arial, Montserrat, Poppins, Playfair Display, Space Grotesk...), une échelle typographique de 5 à 7 tailles (h1 d'au moins 3rem, corps 16 à 18px, interligne 1,5 à 1,7, lignes de 45 à 75 caractères), une échelle d'espacement en multiples de 8px, une composition qui change d'une section à l'autre, une maquette ASCII du premier écran, un élément mémorable, les **interactions** que le sujet appelle, prises dans l'outil `Component` (une réservation pour des cours, les horaires en direct et l'itinéraire pour une boutique, un filtre pour un catalogue, un panier pour des produits ; deux ou trois, pas toutes), et une idée de **mouvement** mesurée (une entrée au premier écran, quelques apparitions au défilement, des chiffres qui montent, un seul effet de défilement). Seuls `transform` et `opacity` s'animent : 150 à 250 ms au survol, 400 à 700 ms pour une entrée, rien ne tourne en boucle sauf un détail lent, et aucun contenu n'attend un script pour se voir. Les marques d'une page générée (bandeau uni avec « Bienvenue » centré, cartes identiques, trait sous chaque titre, badges en capitales, emoji à la place des photos, fondu sur chaque section, dégradés partout...) sont **interdites** sauf demande du brief, et le modèle doit écrire qu'il n'en a utilisé aucune. Les emoji sont interdits sans aucune exception : chaque icône et pictogramme vient de l'outil `Icon`, chaque forme décorative de l'outil `Ornament`, chaque photo du sujet de l'outil `Photo`, et un emoji plafonne la note de finition du juge à 4. Dès qu'un fichier d'interface écrit en contient un, Claw le dit au modèle avec sa ligne, avant même de regarder la page (les flèches, puces, étoiles de notation et signes légaux ne comptent pas). Viennent ensuite un plancher de qualité (variables CSS pour chaque couleur, taille et espace ; un seul h1 ; menu replié au téléphone qui s'ouvre vraiment ; en-tête fixe qui ne cache rien ; états hover, focus-visible et active sur chaque élément interactif ; cibles de 44px au téléphone ; champs étiquetés au style de la page, erreurs après la saisie seulement ; `prefers-reduced-motion`), des liens et boutons qui mènent quelque part, puis la vérification par `Screenshot`, et une auto-notation stricte de 1 à 10 sur sept critères avant de répondre : tout critère sous la barre (`CLAW_DESIGN_BAR`, 8 par défaut) est à relever d'abord. La méthode donne aussi un **socle de code à recopier tel quel** : le lien Google Fonts au bon format (`css2?family=` : une autre forme ne charge rien, et la page tombe en Times et Arial), `box-sizing`, un conteneur, `min-width: 0` sur les éléments des grilles, la police héritée des champs, les états hover, focus-visible et active, `:user-invalid`, `[hidden]`, `prefers-reduced-motion`, et les pièges vus dans les essais (une version masquée par une media query sans l'autre, une largeur en `vw`). Les revues portent ainsi sur le design, plus sur ces réparations. Les icônes viennent de l'outil `Icon`, et ne se dessinent jamais à la main, pas même une flèche, une coche, une croix ou la flèche d'une liste déroulante en adresse `data:` (voir « Icônes jamais dessinées à la main ») ; les formes décoratives (galets, vagues, arches, médaillons, rameaux, soleils, textures de points, de lignes, de courbes de niveau ou de grain) de l'outil `Ornament`, tracées par formules aux couleurs du plan ; les photos du sujet de l'outil `Photo`. Une illustration que le modèle dessine encore lui-même reste simple et géométrique. Les juges le savent : ils proposent une photo par ce qu'elle doit montrer, ou un ornement par sa sorte et sa place, plutôt qu'une scène dessinée à la main. Vu dans les essais réels : les illustrations faites main (des arches en forme de cuillère, des avatars en goutte, une vague coupée) laissaient l'identité et la finition sous la barre. **Avant le code**, quand le modèle a posé son plan (au moins trois couleurs en hexadécimal) et s'apprête à écrire un fichier d'interface, un autre modèle juge la direction (identité, hiérarchie, typographie, couleur, composition) : cinq changements au plus, ou rien à redire, joints au résultat de l'écriture. Changer un plan ne coûte rien, changer une page faite coûte une réécriture : une requête de plus, une fois par tour. **Un tour de design a 160 requêtes** au lieu de 100, sauf si vous avez réglé la limite vous-même (`turnRequestLimit`, `CLAW_TURN_REQUESTS`) : plan, code, captures et revues dépassaient le budget ordinaire, et la deuxième revue n'avait jamais lieu. Avec `designModel` (voir `/models bench --design`), le tour de design est confié à ce modèle, et le modèle de la session reprend à la fin du tour | `CLAW_DESIGN_GUIDE=0` |
| Revue visuelle | Le modèle a modifié l'interface (HTML, CSS, JS, composants) et dit avoir fini : Claw regarde la page comme un utilisateur. Il prend des captures pleine page au téléphone et à l'ordinateur (la page que le modèle a regardée, sinon le `index.html` modifié), relève les défauts mesurés par `Screenshot` et les erreurs de la console, puis fait **noter** les captures par un modèle gratuit qui voit, en directeur artistique difficile : une note de 1 à 10 sur sept critères (téléphone, hiérarchie, espacement, typographie, couleur, identité, finition ; 5 un gabarit correct, 8 ce qu'un bon studio signe, 10 digne d'un prix), puis, pour chaque critère sous la barre (`CLAW_DESIGN_BAR`, 8 par défaut, de 5 à 10), les changements exacts qui le relèvent (propriétés CSS, structure, texte), huit au plus. À partir de la deuxième revue, le juge reçoit ses notes précédentes comme repère : il garde la même échelle, et un écart de deux points ou plus doit être justifié par ce qu'il voit (vu dans un essai réel, le même juge avait noté 7,9 puis 6,0 une page qui s'était améliorée). Un « rien à changer » du juge ne compte pas tant qu'une note reste sous la barre, et les remarques de design mesurées sont à corriger, pas seulement à reconsidérer. Le tout repart au modèle, qui corrige et vérifie de nouveau ; vous voyez la note (« 6,4/10, barre 8, sous la barre : identité, finition »). À la dernière revue du tour, le modèle doit vous donner les notes et dire ce qui reste sous la barre. Trois revues au plus par tour (`CLAW_DESIGN_REVIEW_ROUNDS`, 5 au plus) ; sautée quand il reste moins de 25 requêtes gratuites dans la journée. Quand le tour a dépensé les trois cinquièmes de son budget de requêtes (`turnRequestLimit`) sans qu'une interface modifiée ait été revue, la revue a lieu tout de suite, pour que les corrections tiennent dans le budget : à la limite, la conclusion part sans aucun contrôle. Vu dans un essai réel : un modèle a consommé ses 100 requêtes à se corriger seul, et la limite a emporté la revue avec elle. **Poursuite jusqu'à la barre** : quand le tour touche sa limite de requêtes ou de revues avec une note sous la barre, il continue, de 60 requêtes ou d'une revue de plus, tant que la note monte d'une revue à l'autre (de 0,2 point au moins). Chaque poursuite demande une nouvelle revue notée ; deux au plus par tour (`CLAW_DESIGN_CONTINUE`, 0 les coupe, 5 au plus), aucune quand il reste moins de 150 requêtes gratuites dans la journée. Vous le voyez (« Poursuite du design (1/2, limite du tour) : note 6,0/10, sous la barre 8 (identity, finish) ; le tour continue »), et quand le tour s'arrête sous la barre, vous lisez pourquoi (« Design arrêté sous la barre 8 (note 6,0 puis 6,1/10) : la dernière revue n'a pas fait monter la note ») | `CLAW_DESIGN_REVIEW=0` |
| Photos libres | L'outil `Photo` cherche des photos sous licence libre (CC0, domaine public, CC BY, CC BY-SA) par mots anglais, sur Openverse puis Wikimedia Commons, sans clé ni compte, et les enregistre dans le projet (`images/` par défaut) avec leur taille réelle et un crédit à mettre en pied de page, aussi écrit dans `images/CREDITS.md`. Chaque photo suit le résultat en image, comme une capture : le modèle (ou le lecteur d'images, pour un modèle qui ne les lit pas) juge si elle montre bien le sujet ; vu dans un essai réel, « yoga teacher portrait » a donné une séance de Pilates sur machine. Une recherche qui ne trouve rien est reprise avec ses trois puis ses deux premiers mots : chaque mot doit figurer dans la photo, et « yoga studio interior natural light wooden floor » ne trouvait rien là où « yoga studio interior » donnait dix photos. Ce qui vient de ces services est traité comme une donnée étrangère : un fichier n'est gardé que s'il est bien une image JPEG, PNG ou WebP de 8 Mo au plus, en `https`, écrit dans un dossier du projet et jamais ailleurs ; titres et auteurs sont nettoyés et échappés. La méthode de design demande de regarder chaque photo dans la page : une photo qui ne va pas au sujet est pire qu'aucune. Chaque photo du résultat porte aussi sa petite version chez sa source (`web_thumbnail` : carré de 150 pixels de Flickr, miniature de 120 pixels de Commons, une de ses tailles standard) et son adresse d'origine (`web_original`) : Claw Web en fait des vignettes dans le journal du passage, que le modèle ne doit jamais lier dans la page. La vignette d'Openverse n'est pas prise : son serveur répondait en 30 s par une erreur 500 | `CLAW_PHOTOS=0` |
| Annonce du plan | Les consignes demandent au modèle, dans sa première réponse à une demande et avant son premier appel d'outil, de dire à l'utilisateur en une à trois phrases, dans sa langue, ce qu'il va faire et dans quel ordre, puis une phrase à chaque nouvelle étape. S'il lance ses premiers outils sans un mot, la requête suivante part **sans outils** (`tool_choice: none`) : le modèle ne peut répondre que par ces quelques phrases, puis le travail reprend. De même après 20 étapes d'outils d'affilée sans un mot : il dit où en est le travail. Une requête de plus à chaque fois, jamais pour un sous-agent ; une annonce qui échoue ou reste vide est simplement sautée. Vu dans une vraie session : Nemotron 3 Super ne mettait son plan que dans son raisonnement, en anglais, et une note jointe aux résultats des outils restait sans effet ; avec la requête sans outils, il l'annonce en français dès la deuxième requête. Claw Web montre ce plan en tête de la progression (« Ce que Claw compte faire », puis « En ce moment ») | `CLAW_ANNOUNCE=0` |
| Question sur le travail | Un message court qui demande ce que fait l'agent, ce qu'il a fait ou pourquoi (« Tu fais quoi !? », « Pourquoi tu as fait ça ? », « Où en es-tu ? », « What are you doing? »), sans demande de travail (« … Remets-le » en est une), reçoit une réponse et rien d'autre : la première requête du tour part sans outils, et sa réponse termine le tour. Vu le 20 octobre 2026 dans une session de Claw Web : à « Tu fais quoi !? », Nemotron 3 Ultra avait relancé tous les tests, écrit un nouveau fichier et répondu sur les remarques de son relecteur (65 actions, 12 minutes) ; à l'essai, il répond maintenant en une requête et 7 secondes. Les remarques des contrôles de Claw (relecteur, vérification) sont aussi dites venir de Claw et non de l'utilisateur : la réponse finale répond au message de l'utilisateur et ne mentionne leurs corrections qu'en une phrase | `CLAW_WORK_QUESTIONS=0` |

**Mode équipe : le bon modèle pour chaque étape.** Au lieu d'un seul modèle pour tout, Claw
confie chaque étape du travail au modèle gratuit le plus fort pour elle, et change de modèle
de lui-même selon l'avancement et le sujet :

| Étape | Quand | Modèle choisi |
|---|---|---|
| Comprendre et préparer | début du tour, aucun fichier encore modifié | le plus proche de Claude (Fable d'abord) : raisonnement, outils, contexte, code, mesures de Claw |
| Écrire le code | des fichiers ont été modifiés | le plus efficace pour le code (classement du code et mesures de `claw models bench`) |
| Déboguer | une vérification ou une commande de test a échoué, deux outils de suite en échec, ou une remarque d'un contrôle de Claw attend sa correction | le plus proche de Claude |
| Relire (vérificateur ultracode) | à la fin, avec ultracode | le plus proche de Claude, mais jamais celui qui a écrit le code |
| Images | la demande contient une image | seulement un modèle qui lit les images |
| Sujet spécialisé | une demande hors programmation sur la traduction, le droit, la finance, la santé, les sciences, les études ou le marketing | le mieux classé de la catégorie chez OpenRouter, sinon un modèle dont c'est la spécialité |

Une longue conversation ne va qu'à un modèle dont le contexte la contient ; un modèle mis de côté
(quota, panne) laisse la place au suivant. Les choix viennent du catalogue en cache : aucune
requête de plus. Sans meilleur choix, le modèle de la session garde la main, et il reprend à la
fin de chaque tour.

Pour que chacun sache ce qu'il a à faire, chaque modèle reçoit toute la conversation (demande,
plan, liste de tâches, résultats des outils) et une consigne de relais : il est un membre de
l'équipe, il reprend le travail des autres là où il en est, sans refaire ce qui est fait, et voici
son rôle pour cette étape. Le raisonnement d'un modèle n'est renvoyé qu'à lui. Chaque changement
s'affiche (« Équipe : « écrire le code » confié à … (le plus efficace pour le code) »). Quand
quelqu'un est là pour répondre, un modèle de l'équipe qui tombe en panne fait poser la question
habituelle (réessayer, changer de modèle) au lieu de passer seul à un autre.

`/equipe` l'active et montre qui fait quoi en ce moment ; `/equipe off` le coupe ; l'interrupteur
**Équipe** de la page et du panneau VS Code fait de même ; `"team": true` ou `CLAW_TEAM=1` pour
chaque session. `"teamModels"` impose un modèle par rôle (`plan`, `code`, `debug`, `review`,
`vision`, ou une catégorie comme `translation`), par exemple un modèle Gemini gratuit pour les
images :

```json
{ "team": true, "teamModels": { "vision": "gemini:gemini-3.8-flash" } }
```

Avec ultracode, l'agent d'exploration prend le modèle « comprendre » et le vérificateur celui
« relire ». Le mode équipe ne change pas le nombre de requêtes ; il les répartit. Son effet réel
sur la qualité n'a pas encore été mesuré sur de vrais modèles.

**Ultracode : le déroulé le plus poussé.** Inspiré du réglage du même nom de Claude Code, mais
tenu par Claw lui-même plutôt que confié au modèle : un modèle gratuit s'organise mal, s'arrête
trop tôt et juge son propre travail avec indulgence. Pour chaque demande conséquente :

1. **Avant le travail**, un agent d'exploration au contexte vierge, en lecture seule, étudie la
   demande et le code et rédige un dossier : exigences numérotées (y compris ce que la demande
   implique sans le dire), code concerné avec ses lignes, commandes de build et de test du
   projet, risques, plan dont chaque étape dit comment la vérifier. Le dossier est joint à la
   demande ; l'agent principal travaille avec.
2. **Après le travail**, un vérificateur indépendant, qui n'a pas écrit le code et ne voit pas la
   conversation, reçoit la demande, le dossier, le diff du tour et la réponse que l'agent
   s'apprête à donner. Il relit le code de chaque exigence, lance les tests et le vérificateur
   de types, essaie les cas limites, confronte chaque affirmation de la réponse aux fichiers, et
   termine par `VERDICT: PASS` ou `VERDICT: FAIL`. Ses objections repartent à l'agent, puis il
   vérifie de nouveau (trois vérifications au plus par tour). Sans modification de fichier
   (explication, analyse, conseil), il contre-vérifie une fois la réponse dans le code.
3. Le vérificateur remplace le contrôle des exigences que le modèle faisait sur son propre
   travail ; la vérification du projet, la relecture par un autre modèle et la persévérance
   restent actives.

Les deux agents apparaissent comme deux étapes « Sous-agent » dans la page, leur rapport lisible
dans la carte. Ils suivent vos autorisations : le vérificateur peut lancer des commandes, le mode
choisi s'applique. Coût : une à trois dizaines de requêtes de plus par demande conséquente, et
plus de temps ; une relance de moins de quatre mots (« reprendre », « vas-y ») n'est pas étudiée
de nouveau. Pour l'activer :

- pour la session : `/think ultracode` (ou `/effort ultracode`, comme dans Claude Code), ou
  l'interrupteur **Ultracode** à côté du choix du modèle dans la page et le panneau VS Code ; il
  pousse aussi le raisonnement à `max`. `/think ultracode off` le coupe ;
- pour un seul message : écrivez le mot `ultracode` dedans (« ultracode: corrige la connexion ») ;
- pour chaque session : `"ultracode": true` dans `.claw/settings.json`, ou `CLAW_ULTRACODE=1`.
  `CLAW_ULTRACODE=0` (ou `CLAW_HARNESS=0`) l'interdit, et `claw models bench` s'en passe.

Le vérificateur tourne sur le modèle de la session ; `"agents": {"Verification": {"model": "…"}}`
le confie à un autre modèle gratuit, pour un regard vraiment différent.

**Méthode de réflexion.** Les consignes de Claw demandent au modèle de réfléchir avant d'agir :
partir de ce qui est vraiment demandé (chaque partie de la demande), travailler sur des preuves
(lire le code, lancer les tests) plutôt que de mémoire, trouver la cause d'un bogue avant de
modifier quoi que ce soit et chercher le même défaut ailleurs, peser les conséquences de chaque
changement, expliquer un résultat inattendu avant de réessayer, et relire son travail avant de
conclure. Elles tirent aussi les leçons d'une vraie session où le modèle avait été trop mou : un
échec est une information et non une raison de vous rendre le travail ; faire au lieu d'annoncer
(pas de tour qui se termine sur un plan ou une liste d'étapes qu'il pouvait mener lui-même) ;
obtenir toutes les erreurs d'un coup (vérificateur de types, compilation, tests) au lieu de les
découvrir une compilation à la fois ; vérifier le comportement et pas seulement que le code se
charge ; n'affirmer dans la réponse finale que ce que les fichiers et les commandes montrent.
Le raisonnement doit servir à décider (faits, hypothèses, preuves, décision), pas à raconter ce
qui va suivre. Avant la première modification, le modèle balise le terrain (ce que fait le code
aujourd'hui, tout ce que le changement touche, l'ordre du travail, la preuve qu'il marchera) ;
devant un problème subtil, il suit l'exécution réelle pas à pas avec les valeurs en jeu ; un
travail de plus de deux étapes est découpé en tâches (`TodoWrite`), et une tâche ouverte veut
dire que le travail n'est pas fini.

**Des réponses approfondies.** Quand vous demandez d'expliquer, d'analyser, de comparer, de
relire, de concevoir ou de conseiller, la réponse est le travail lui-même : chaque point appuyé
sur le code lu (fichiers, fonctions, lignes), les mécanismes et leurs raisons, les options avec
leurs compromis, les risques et cas limites, puis une recommandation claire. Le compte rendu
d'un travail explique la cause de chaque problème (pourquoi, ce qu'il touchait, la preuve), et
pourquoi telle solution plutôt qu'une autre quand le choix comptait. Une question simple garde
une réponse directe.

**Le plus haut niveau de réflexion de chaque modèle.** Sans réglage, un modèle OpenRouter qui
sait raisonner reçoit l'effort `max` : OpenRouter le ramène au plus haut niveau que ce modèle
accepte (`high` pour Nemotron 3 Ultra, `max` pour Inkling). Seule exception, un modèle qui ne
raisonne que par budget de jetons reçoit `high` : avec `max`, 95 % de la place de sa réponse
partirait en réflexion et ses appels d'outils seraient coupés. Si une réponse s'arrête quand
même à la limite de sortie sans texte ni appel (la réflexion a tout pris), ce modèle descend d'un
cran pour la suite de la session, et Claw le dit. La limite de sortie suit le catalogue jusqu'à
65 536 jetons : le raisonnement compte dedans. Les niveaux acceptés par chaque modèle viennent
du catalogue d'OpenRouter, rafraîchi en arrière-plan au démarrage quand le cache est d'une
version antérieure (lecture publique, sans clé ni quota).

**Une réponse finale en forme de compte rendu.** Le modèle termine par une seule réponse,
écrite pour vous qui n'avez pas suivi ses étapes : le résultat d'abord, puis ce qui n'allait pas
et pourquoi, la solution apportée à chaque problème, ce qui a été vérifié (et ce qui ne l'a pas
été), et ce qu'il vous reste à faire. Une question simple reçoit une ou deux phrases, pas un
rapport.

**Une seule réponse finale.** Quand un contrôle renvoie le modèle au travail (abandon sur des
échecs, vérification en échec, erreurs de l'éditeur, exigences, relecture), la réponse qu'il venait de donner n'était que
provisoire. Claw lui demande alors de terminer par une réponse complète pour vous : ce qui a été
fait, et tout ce que vous aviez demandé d'expliquer (comment installer, lancer ou tester), sans
parler du contrôle que vous n'avez pas vu. Dans l'interface web et le panneau de VS Code, les
réponses provisoires sont repliées (« Réponse provisoire, reprise après un contrôle de Claw »,
un clic les affiche) : la dernière réponse du tour est celle à lire. Les messages que Claw
adresse au modèle (« [Claw] … ») n'apparaissent pas comme des messages de votre part quand une
session est reprise.

`CLAW_HARNESS=0` les coupe toutes. Réglages dans `.claw/settings.json` ou `~/.claw/settings.json` :

```json
{
  "verify": "cargo test --quiet",
  "review": true,
  "advisorModels": ["nvidia/nemotron-3-ultra-550b-a55b:free", "cohere/north-mini-code:free"],
  "plannerModel": "nvidia/nemotron-3-ultra-550b-a55b:free",
  "escalationModel": "free",
  "designModel": "nvidia/nemotron-3-super-120b-a12b:free",
  "ultracode": true,
  "turnRequestLimit": 100
}
```

- `"verify"` : une commande, une liste de commandes, ou `false`. Sans réglage, Claw la déduit du
  projet. Dans une session non interactive, la commande doit être autorisée par une règle
  `permissions.allow` ; sinon Claw annonce « Vérification non lancée […] : le travail n'est pas
  vérifié ».
- `"advisorModels"` : modèles consultés pour la relecture, le conseil et le second avis, du
  préféré au moins préféré. Sans réglage : avec une clé NVIDIA, ses modèles autres que celui de
  la session, du plus puissant au plus léger (aucun quota par jour : Nemotron Ultra relit le
  travail de Lightning) ; avec une clé OpenRouter, ensuite, les meilleurs modèles gratuits
  autres que celui de la session (`claw doctor` les affiche, ligne « Autres avis »). Un
  conseiller surchargé, muet pendant 20 s, trop long à finir (50 s) ou qui répond sans texte
  laisse la place au suivant ; une consultation dure 90 s au plus.
- `"plannerModel"` : un modèle fort pour réfléchir, un modèle gratuit pour exécuter. Claw l'annonce
  au début du tour (« Mode plan : la réflexion est confiée à … »).
- `"turnRequestLimit"` (ou `CLAW_TURN_REQUESTS`) : le nombre de requêtes au modèle qu'un même tour
  peut faire, 100 sans réglage, `0` pour ne pas borner. Arrivé là, Claw vous prévient, demande à
  l'agent de dire ce qui est fait, ce qui est vérifié et ce qui reste, n'exécute plus aucun outil
  et vous rend la main : écrivez « continue » pour poursuivre, avec un budget entier. Un modèle
  qui tourne en rond ne peut plus dépenser en un seul tour le quota gratuit de la journée (vu
  dans un essai réel : 250 requêtes en 25 minutes, sans plus rien modifier après la sixième).
  Les aides coupées par `CLAW_HARNESS=0` n'y changent rien : c'est une limite, pas une aide.
- `"escalationModel"` (ou `CLAW_ESCALATION_MODEL`) : qui reprend un tour bloqué. Sans réglage, ou
  avec `"free"` : **le modèle gratuit le plus puissant** du classement de `/models` (taille, rang
  en programmation, spécialisation, mesures de `/models bench`), choisi au moment du blocage
  parmi les modèles disponibles, dans le catalogue déjà en cache. Si le modèle de la session est
  déjà le plus puissant, il n'y a pas d'escalade. `"off"` la coupe ; un identifiant de modèle
  impose ce modèle-là (un payant ne sert donc que si vous l'écrivez vous-même). `/models` montre
  le choix du moment, ligne « Escalade ». Claw l'annonce (« L'agent n'avance plus : … reprend la
  fin du tour ») ; s'il est indisponible, la chaîne habituelle prend le relais. `/models bench`
  mesure chaque modèle seul, sans escalade.
- `"designModel"` (ou `CLAW_DESIGN_MODEL`) : le modèle des tours de design, ceux où la méthode de
  design est jointe. Claw l'annonce (« Tour de design confié à … ») et le modèle de la session
  reprend à la fin du tour ; s'il est indisponible, la chaîne habituelle prend le relais.
  `/models bench --design --appliquer` l'écrit pour vous ; `"off"` le coupe.

Les avis des autres modèles ne sont jamais des ordres : ils reviennent au modèle de la session
comme des remarques à confronter au code. Les diagnostics d'un serveur de langage (LSP) ne sont
pas branchés : le contrôle de syntaxe après chaque modification et le vrai compilateur en fin de
tour couvrent ce besoin sans diagnostics en retard.

### Trouver les meilleurs modèles gratuits compatibles : `claw models bench`

Le classement de `/models` repose sur des signaux indirects (rang OpenRouter, spécialisation
citée). `claw models bench` (ou `/models bench` dans le REPL et l'interface web) **mesure** :

1. **Sonde de compatibilité** de chaque modèle gratuit qui appelle des outils (≈ 3 requêtes) : il
   doit lire un fichier avec `read_file` et en citer le code secret. Écarte les modèles refusés à
   Claw (403), introuvables (404), trop lents, qui n'appellent pas les outils ou répondent faux ;
   un modèle seulement surchargé ou limité par son fournisseur est noté « ‖ indisponible pour
   l'instant » sans être écarté.
2. **Trois tâches de code** sur les 4 meilleurs modèles compatibles (corriger un bug, implémenter
   une fonction, renommer dans plusieurs fichiers), vérifiées par les tests du projet (Python).
   Une tâche que le modèle n'a pas pu faire faute de disponibilité n'est pas comptée comme un
   échec, et le modèle suivant prend sa place.

Options : `--rapide` (sonde seulement), `--local` (ajoute Ollama, LM Studio…), `--max N` (modèles
qui passent les tâches), `--appliquer` (règle le meilleur comme `"model"` dans
`~/.claw/settings.json`, avec `"fallbackModels": ["free", …modèles locaux déjà prévus]`), et un
filtre (`claw models bench cohere`). Les résultats sont gardés 14 jours dans
`~/.claw/model-bench.json` : `/models` les affiche, le choix automatique du modèle gratuit et le
mot-clé `"free"` de `fallbackModels` mettent les modèles mesurés en tête et écartent les
incompatibles. Chaque essai lance `claw prompt` dans un projet temporaire où `Bash(python:*)`
est autorisé (sans règle `permissions.allow`, une tâche non interactive ne peut lancer aucune
commande).

**Tâches de raisonnement** : `claw models bench --difficile [modèle]` fait passer six tâches plus
dures au modèle par défaut (ou à celui du filtre), vérifiées par des **tests cachés** que le
modèle ne voit jamais : une cause située dans un autre fichier que celui du symptôme, les cas
limites d'une spécification, corriger sans toucher aux tests, un réglage à faire suivre dans
quatre fichiers, un code correct mais trop lent, un évaluateur d'expressions. Quand la consigne
interdit de toucher aux tests, les modifier est un échec ; dans tous les cas les tests visibles
d'origine sont remis en place avant la vérification (les affaiblir ne sert à rien, en ajouter
n'est pas une faute). `--sans-aide` rejoue les mêmes tâches sans les aides au
raisonnement (`CLAW_HARNESS=0`) pour comparer ; les deux scores sont gardés, et celui obtenu avec
les aides compte double dans le classement des modèles. Une tâche que le modèle n'a pas pu
traiter (surcharge, flux coupé) est réessayée une fois. Environ 80 requêtes par série, 10 minutes
au plus par tâche.

Mesure du 9 octobre 2026, une passe par série : Claw avec `cohere/north-mini-code:free`, sans
puis avec les aides, et **Claude Code** (`claude -p`, modèle Opus 5) sur les mêmes tâches, dans
les mêmes conditions (fichiers visibles seulement, même consigne, Python autorisé, 600 s au plus).

| Tâche | Claw sans aide | Claw avec les aides | Claude Code |
|---|---|---|---|
| Cause dans un autre fichier | réussie, 74 s | réussie, 79 s | réussie, 92 s |
| Cas limites d'une spécification | échec (2 tests cachés) | échec (2 tests cachés) | réussie, 54 s |
| Corriger sans toucher aux tests | réussie, 462 s | réussie, 26 s | réussie, 45 s |
| Réglage à faire suivre dans quatre fichiers | réussie, 349 s | réussie, 161 s | réussie, 47 s |
| Correct mais trop lent | réussie, 145 s | réussie, 89 s | réussie, 108 s |
| Priorités, parenthèses, moins unaire | réussie, 600 s (délai atteint) | réussie, 273 s | réussie, 99 s |
| **Réussite** | **5/6** | **5/6** | **6/6** |
| **Durée des cinq tâches réussies par tous** | 1 630 s | 628 s | 391 s |
| **Coût** | 0 $ (requêtes gratuites) | 0 $ (requêtes gratuites) | ≈ 2,50 $ en équivalent API |

- **Les aides** ne changent pas le taux de réussite de ce modèle mais divisent le temps de
  travail par 2,6 ; une première mesure le même jour allait dans le même sens (médiane 162 s
  contre 235 s).
- **Claude Code** réussit les six tâches et va 1,6 fois plus vite que Claw avec ses aides sur
  les cinq tâches communes. La tâche « cas limites », que ce modèle gratuit rate trois fois sur
  cinq, est son vrai point faible : ni la vérification ni la relecture ne le rattrapent, parce
  que les tests visibles passent.
- Les durées varient beaucoup d'une passe à l'autre (462 s puis 26 s pour une même tâche) : une
  seule passe donne un ordre de grandeur, pas un classement fin. Ces tâches sont petites (un à
  quatre fichiers Python) ; l'écart dû au modèle se creuse sur un vrai projet.

**Design** : `claw models bench --design [--max N] [--appliquer] [modèle]` cherche le modèle
gratuit qui fait les plus belles pages. Chaque candidat (les trois mieux classés qui marchent avec
Claw, ou ceux du filtre) construit la même page d'accueil, celle d'un fleuriste lyonnais, avec la
méthode de design mais sans revue ni autre modèle (on mesure le modèle, pas le juge), en 60
requêtes au plus. Claw capture la page au téléphone et à l'ordinateur et la fait noter par le
juge qui voit, sur les sept critères de la revue visuelle ; un modèle n'est jamais jugé par
lui-même. Le rapport classe les modèles par note, avec les critères sous la barre, les défauts
mesurés, la durée et le juge ; chaque page est gardée (`design-bench/<modèle>/` dans le dossier
de configuration) pour la regarder. `--appliquer` écrit le meilleur dans `designModel`. Environ
60 requêtes et 10 à 20 minutes par modèle ; une note varie d'un essai à l'autre, un écart de
moins d'un point ne départage pas vraiment.

Mesures sur un PC portable (i7-8750H, 16 Go, GTX 1050 4 Go, Ollama 0.40) : un modèle local de
4 milliards de paramètres (`qwen3.5:4b`) réussit la plupart des tâches mais en 1 à 10 minutes
chacune, `qwen2.5-coder:7b` n'appelle aucun outil. En ligne, `cohere/north-mini-code:free` a
réussi toutes les tâches en quelques dizaines de secondes.

### Compatibilité avec Claude Code

Claw lit aussi `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md`, `AGENTS.md` (et
`~/.claude/CLAUDE.md`, `~/.claw/CLAW.md`), les agents de `.claude/agents/`, les compétences de
`.claude/skills/`, les commandes de `.claude/commands/`, et dans `.claude/settings.json`,
`.claude/settings.local.json`, `~/.claude/settings.json` et `.mcp.json` les clés `permissions`,
`hooks` et `mcpServers` (les fichiers de Claw l'emportent). Un agent Claude Code déclaré avec
`model: sonnet` prend le modèle de la session si vous n'avez pas de clé Anthropic.
`CLAW_NO_CLAUDE_CONFIG=1` ignore ces fichiers.

**Mémoire du projet** (comme dans Claude Code) :

- **Imports** : `@chemin` dans `CLAUDE.md`, `CLAW.md`, `AGENTS.md` ou une règle charge ce fichier
  en plus (`See @README`, `@docs/git.md`, `@~/.claude/mes-preferences.md`, `@Design\ Docs/api.md`
  pour un espace). Chemin relatif au fichier qui importe, quatre niveaux d'imports au plus,
  ignoré dans le code (`` `@x` ``, blocs). Un import d'un fichier de projet qui sort du projet
  (un fichier personnel par exemple) n'est chargé qu'après `/memory imports allow` : un
  fichier commité par d'autres ne peut pas faire lire au modèle des fichiers de votre machine.
  Les imports de vos fichiers personnels (`~/.claude/CLAUDE.md`, `~/.claw/CLAW.md`) passent.
- **Commentaires** : les blocs `<!-- … -->` sont retirés avant l'envoi au modèle (hors code).
- **Règles** : un fichier Markdown par sujet dans `.claude/rules/` ou `.claw/rules/` (sous-dossiers
  acceptés, liens symboliques suivis), et `~/.claude/rules/` pour tous vos projets. Sans en-tête,
  une règle est chargée au démarrage ; avec `paths` (`"src/api/**/*.ts"`, accolades acceptées :
  `"src/**/*.{ts,tsx}"`), elle est donnée au modèle la première fois qu'il lit ou modifie un
  fichier correspondant.
- **Sous-dossiers** : le `CLAUDE.md` (ou `CLAW.md`, `AGENTS.md`) d'un sous-dossier est donné au
  modèle la première fois qu'il touche un fichier de ce sous-dossier.
- **`claudeMdExcludes`** (réglages, aussi lu depuis `.claude/settings.json`) : globs des fichiers
  d'instructions à ignorer, par exemple ceux d'autres équipes dans un monorepo.
- **`/memory`** liste les fichiers chargés (imports compris), les règles par chemin et les imports
  externes en attente ; `/memory imports allow|deny` décide pour le projet.

**Réglages généraux** (mêmes noms que dans Claude Code ; sauf `env`, ils sont aussi lus depuis
`.claude/settings.json`) :

- **`env`** : variables d'environnement appliquées au démarrage, pour Claw et tout ce qu'il lance
  (bash, hooks, serveurs MCP). Une valeur du réglage remplace celle du shell ; `""` la vide.
  Un fichier de projet (`.claw/settings.json`, `.claw/settings.local.json`) ne peut pas fixer
  les clés et adresses des fournisseurs (`*_API_KEY`, `*_TOKEN`, `*_BASE_URL`, proxys),
  `PATH`, `HOME`, les dossiers temporaires ou système : un dépôt cloné ne doit pas pouvoir
  envoyer vos clés ailleurs. Mettez-les dans `~/.claw/settings.json` ou le shell ;
  `/config env` liste celles qui ont été ignorées.
- **`attribution`** : texte ajouté à la fin des commits (`commit`, par défaut
  `Assisted-by: Claw Code`) et des descriptions de PR (`pr`, par défaut `Generated with Claw Code`) ;
  `""` retire l'un, `"attribution": false` retire les deux. L'ancien `includeCoAuthoredBy: false`
  est toujours compris. `/commit`, `/commit-push-pr` et `/pr` l'ajoutent eux-mêmes ; quand le
  modèle fait un commit sans cette ligne, Claw le lui signale pour qu'il corrige le commit
  avant de le pousser. Vos consignes (`CLAW.md`, `CLAUDE.md`) l'emportent.
- **`includeGitInstructions: false`** (ou `CLAW_DISABLE_GIT_INSTRUCTIONS=1`) retire du prompt
  les consignes de commit et de PR et l'état git du dépôt, par exemple si vos propres skills
  gèrent git.
- **`cleanupPeriodDays`** : au début d'une session, les sessions et points de restauration du
  projet plus anciens que ce nombre de jours sont supprimés. Sans ce réglage, Claw garde tout
  (Claude Code, lui, supprime au bout de 30 jours par défaut).
- **`companyAnnouncements`** : annonces de l'équipe ; l'une d'elles, prise au hasard, s'affiche
  au démarrage.
- **`apiKeyHelper`** : une commande fournit la clé d'API (jeton de courte durée tiré d'un coffre,
  gestionnaire de mots de passe). Sa première ligne de sortie sert de clé quand la variable du
  fournisseur (`OPENROUTER_API_KEY`…) est absente ; elle est gardée 5 minutes
  (`CLAW_API_KEY_HELPER_TTL_MS`) et redemandée après un refus 401 ou 403. Réglée dans
  `~/.claw/settings.json`, elle vaut pour le fournisseur du modèle de la session ; dans
  `~/.claude/settings.json`, pour `ANTHROPIC_API_KEY` seulement ; `providers[].apiKeyHelper` la
  règle par fournisseur (`apiKeyEnv`). Un fichier de projet ne peut pas en régler, et une clé
  n'est jamais donnée à un autre fournisseur que le sien.

**Compétences (skills)** : un dossier `<nom>/SKILL.md` placé dans `.claude/skills/` (ou
`.claw/skills/`, `.codex/skills/`) du projet, d'un dossier parent ou du dossier personnel. Claw
donne au modèle le nom et la description de chacune (40 au plus) ; quand une demande y
correspond, le modèle charge ses instructions avec l'outil `Skill` et lit les fichiers qui
l'accompagnent. `claw skills` (ou `/skills`) les liste. Une compétence marquée
`disable-model-invocation: true` ne se lance que par `/nom`.

Les skills suivent le format de Claude Code :

- **`/nom arguments`** lance le skill (le nom du dossier ou le `name` du frontmatter) ; un skill
  passe avant une commande de `commands/` de même nom. Le menu `/` et la complétion les listent.
- **Arguments** : `$ARGUMENTS` (tout le texte), `$ARGUMENTS[0]` ou `$0` (premier argument, `$1`
  le deuxième…), noms déclarés dans `arguments: [issue, branch]` (`$issue`, `$branch`). Les
  guillemets groupent un argument (`/migrer "Search Bar" JS TS`) ; sans aucun emplacement, le
  texte est ajouté en `ARGUMENTS: …` ; `\$1` reste littéral. Variables : `${CLAUDE_SKILL_DIR}`,
  `${CLAUDE_PROJECT_DIR}`, `${CLAUDE_SESSION_ID}`, `${CLAUDE_EFFORT}` (et `${CLAW_…}`).
- **Contexte dynamique** : `` !`git diff HEAD` `` (en début de ligne ou après un blanc) et les
  blocs ` ```! ` sont exécutés avant l'envoi et remplacés par leur sortie. Chaque commande passe
  les permissions de la session sans jamais demander : une commande non autorisée d'avance
  (lecture seule, règle `allow`, `allowed-tools` du skill) annule l'invocation, comme une commande
  en échec (le code 1 de `grep`, `diff`, `git diff`… compte comme un résultat).
  `"disableSkillShellExecution": true` les remplace par `[shell command execution disabled by
  policy]` ; jamais exécutées pour un skill synchronisé de claude.ai.
- **`allowed-tools`** : outils accordés sans demande pendant le tour qui invoque le skill (ex.
  `Bash(git add *) Bash(git commit *)`), sans restreindre les autres ; **`disallowed-tools`** :
  outils refusés pendant ce tour. Les règles `deny` et `ask` restent prioritaires.
- **`model`** et **`effort`** : modèle et niveau de raisonnement le temps du tour.
- **`context: fork`** (avec `agent: Explore`, `Plan`, `general-purpose` ou un agent de
  `.claude/agents/`) : le skill devient la demande d'un sous-agent ; son rapport s'affiche et
  rejoint la conversation.
- **`user-invocable: false`** : réservé au modèle ; **`when_to_use`** complète la description ;
  **`paths`** est signalé au modèle (« only for files matching … »).
- **`skillOverrides`** dans les réglages (aussi lu depuis `.claude/settings.json`) : `"on"`,
  `"name-only"` (seul le nom est donné au modèle), `"user-invocable-only"` (caché au modèle),
  `"off"` (caché partout), sans modifier le fichier du skill.
- Invoqué par le modèle (outil `Skill`), un skill est rendu de la même façon ; un skill réservé à
  l'utilisateur lui est refusé avec la consigne de vous proposer `/nom`.

Claw importe aussi :

- les compétences **synchronisées par l'application Claude** (`~/.claude/skills/synced/…`, un
  dossier par compte, le plus récent l'emporte) ;
- celles des **plugins de Claude Code** installés et activés (`installed_plugins.json` et
  `enabledPlugins` des réglages de Claude Code), sous le nom `plugin:compétence`.

Elles passent après celles des dossiers ci-dessus en cas de nom commun. Celles qui ne peuvent
pas fonctionner dans Claw sont listées par `/skills` avec la raison, mais pas proposées au
modèle : champ `compatibility` réservé à l'application de bureau Claude (navigateur intégré,
contrôle de l'ordinateur), outils MCP que Claw n'a pas (`mcp__serveur__…` sans serveur de ce
nom dans la configuration), ou connecteurs de claude.ai (`docs`, `google-workspace`, `morning`,
`setup-writing-style`). Pour forcer l'une d'elles, copiez son dossier dans `~/.claw/skills/`.
Les compétences de documents (`pdf`, `docx`, `xlsx`, `pptx`) lancent des scripts Python : leurs
bibliothèques (pypdf, python-docx, openpyxl…) doivent être installées. `CLAW_NO_CLAUDE_CONFIG=1`
coupe cet import.

### Images et raisonnement

- **Images** : `@capture.png` (ou un chemin d'image glissé dans le terminal) joint l'image à la
  demande ; dans l'interface web, collez ou glissez une capture. `/models vision` liste les modèles
  gratuits qui lisent les images. Format png, jpg, gif ou webp, 5 Mo maximum.
- **Lecteur d'images** : un modèle d'OpenRouter qui ne lit que du texte refuserait toute la
  requête (erreur 404) dès qu'une image figure dans la conversation. Claw ne lui envoie donc pas
  l'image : il la fait décrire par le modèle gratuit le plus puissant qui lit les images (type
  d'image, disposition, et tout le texte visible recopié mot pour mot : erreurs, code, valeurs),
  et donne cette description au modèle de la session, qui sait qu'elle vient d'un autre modèle.
  Quand un message porte plusieurs images, chaque description est numérotée (« Image 1 of 2 ») :
  deux longues descriptions à la suite se lisaient comme une seule image, et la première était
  oubliée.
  Chaque image n'est décrite qu'une fois (une requête gratuite), la description étant gardée
  dans `.claw/image-descriptions/` ; vous êtes prévenu du modèle qui l'a lue. Si aucun modèle
  n'a pu la décrire, le modèle de la session apprend qu'une image manque et peut vous demander
  de la décrire. Les images restent dans la session et repartent telles quelles si vous revenez
  à un modèle qui les lit. `"imageReader"` (ou `CLAW_IMAGE_READER`) choisit le lecteur : un
  identifiant de modèle, ou `"off"` pour ne rien faire décrire. Sans réglage, le lecteur de
  NVIDIA passe d'abord si sa clé est là, puis ceux d'OpenRouter. Les captures que prennent les
  outils (`Screenshot`) sont décrites en une passe, disposition, contenu, aspect et défauts
  visibles puis un verdict, en 3 minutes au plus : la description complète en deux passes de
  vos propres images prenait jusqu'à 12 minutes par capture. Un modèle qui refuse la requête
  (erreur 403, « réservé aux harnais agentiques ») est écarté pour 7 jours, d'une session à
  l'autre, au lieu d'être réessayé à chaque image.
- **Raisonnement** : sans réglage, un modèle d'OpenRouter qui sait raisonner reçoit le plus haut
  niveau de réflexion qu'il accepte (`max`, ou `high` s'il raisonne par budget) : ses réponses
  sont plus lentes, mais plus réfléchies. `/think off|low|medium|high|max` (`max` : le plus haut
  niveau que le modèle accepte ; ou `"reasoning"` dans la config,
  `CLAW_REASONING`) règle l'effort de réflexion des modèles qui raisonnent (DeepSeek-R1, Qwen3,
  GLM, série o d'OpenAI…) et des modèles Claude via l'API Anthropic (raisonnement étendu, blocs
  signés conservés et renvoyés avec les appels d'outils ; pas via OpenRouter). Le raisonnement
  s'affiche à part (une ligne `∴ Réflexion (5 s)` dans le terminal, le texte complet avec
  `CLAW_SHOW_THINKING=1` ; bloc repliable dans l'interface web) ; `off` le masque.

### Travail en parallèle : worktrees, branches et sous-agents isolés

Comme `claude --worktree`, Claw fait travailler plusieurs sessions ou sous-agents sur le même
dépôt sans qu'ils se marchent dessus : chacun dans un **worktree git** (une copie de travail à
part, sur sa propre branche), rangé dans `.claw/worktrees/<nom>`. Ce dossier est ajouté à
`.git/info/exclude` (local : rien ne change dans le dépôt).

- **`claw --worktree <nom>`** (ou `-w <nom>`, nom généré s'il manque) : la session se lance dans
  le worktree `<nom>`, sur la branche `claw/<nom>` (créé s'il n'existe pas, rouvert sinon). À la
  fin, un worktree créé pour la session et resté intact est retiré avec sa branche ; sinon il est
  gardé, avec les commandes pour reprendre (`claw --worktree <nom>`) ou fusionner.
- **Sous-agents isolés** : l'outil `Agent` accepte `isolation: "worktree"`. Le sous-agent tourne
  alors dans un processus `claw` à part, dans un worktree neuf ; à la fin, Claw enregistre ses
  changements dans un commit sur sa branche et dit à l'agent principal comment les relire
  (`git diff HEAD...claw/agent-…`) et les fusionner. Un sous-agent qui n'a rien changé voit son
  worktree retiré. Plusieurs sous-agents isolés lancés dans le même message modifient le code en
  parallèle sans conflit.
- **`.worktreeinclude`** à la racine du dépôt (syntaxe de `.gitignore`) : les fichiers ignorés par
  git qu'il désigne (`.env`, configuration locale…) sont copiés dans chaque nouveau worktree.
- **`/worktree`** : `list`, `add <nom> [branche]`, `remove <nom> [--force]` (la branche `claw/…`
  est supprimée si elle n'a rien apporté), `prune`.
- **`/branch`** : `list`, `create <nom>` (crée et bascule), `switch <nom>`.
- **`/commit-push-pr [contexte]`** : sur la branche principale, crée d'abord une branche
  `claw/<sujet>` ; puis commit (message généré), `git push -u`, et pull request rédigée par le
  modèle (`gh pr create`), ou le lien pour l'ouvrir dans GitHub si `gh` manque.
- **`/tasks`** (alias de `/bashes`) montre aussi les derniers sous-agents : en cours, terminé ou
  en échec, avec le fichier de leur rapport.

### Autres commandes de Claude Code

| Commande | Effet |
|---|---|
| `/plan [demande]` | Passe en mode plan ; avec une demande, la commence aussitôt |
| `/effort [low…max\|xhigh\|auto\|status]` | Niveau de raisonnement (alias de `/think`) |
| `/tasks` | Ce qui tourne pour la session : commandes de fond, sous-agents, sous-tâches, tâches planifiées, sessions en arrière-plan du projet |
| `/subtask <tâche>` | Un sous-agent reprend la conversation et travaille en arrière-plan ; son résultat revient dans la conversation dès qu'il a fini, sans interrompre votre saisie |
| `/workflows [id]` | Exécutions des workflows de la session (phase, agents, durée), détail d'une exécution, `stop <id>`, `save <id> [nom]` ; liste les workflows enregistrés. Voir « Workflows » |
| `/deep-research <question>` | Workflow intégré : recherches web en éventail, sources lues et recoupées, rapport cité qui revient dans la conversation |
| `/workflow-authoring` | Référence pour écrire un workflow |
| `/rewind` | Revient avant un message précédent (comme `Échap Échap`) |
| `/cd <chemin>` | La session continue dans un autre dossier : ses consignes, réglages, permissions et outils s'appliquent (`/add-dir` garde au contraire le dossier actuel) |
| `/stop` | Arrête la session en arrière-plan ouverte avec `claw attach` (conversation et worktree gardés) |
| `/sandbox [docker\|podman\|off]` | État du bac à sable ; conteneur pour la session |
| `/theme [nom]` | `auto`, `dark`, `light`, `dark-daltonized`, `light-daltonized`, `dark-ansi`, `light-ansi` : coloration du code, fond des blocs et couleurs du Markdown ; enregistré dans `~/.claw/settings.json` (`"theme"`) |
| `/color [couleur\|default]` | Couleur des traits de la saisie pour la session |
| `/terminal-setup` | Dans VS Code ou Cursor, règle `Maj+Entrée` pour un saut de ligne (copie de sauvegarde du fichier des raccourcis) |
| `/reload-skills`, `/reload-plugins` | Relit les skills, ou recharge plugins, hooks, serveurs MCP et réglages, sans redémarrer |
| `/usage` (`/stats`) | Coût, crédits OpenRouter et activité de la session |
| `/insights` | Bilan d'usage du projet en HTML (`.claw/insights.html`) : outils, commandes, échecs, pistes |
| `/release-notes` | Notes de version |
| `/simplify [cible]` | Quatre sous-agents relisent les changements (réutilisation, simplicité, efficacité, niveau d'abstraction), puis Claw applique les corrections sans changer le comportement |
| `/verify [quoi]` | Construit et lance l'application pour constater que le changement fait ce qu'il doit |
| `/statusline [description]` | Écrit un script de ligne d'état et règle `statusLine` |
| `/fewer-permission-prompts` | Relève les commandes fréquentes du projet et propose des règles `allow` pour les plus sûres (tests, builds, linters) |

### Tâches planifiées : `/loop` et rappels

Comme dans Claude Code, une demande peut revenir toute seule pendant la session : surveiller
un déploiement, relancer les tests, rappeler quelque chose plus tard. Les tâches passent
**entre deux tours** : la saisie reste libre entre les passages, et un texte en cours de frappe
est remis dans la saisie après le passage.

- `/loop 5m vérifie si le déploiement est terminé` : premier passage tout de suite, puis toutes
  les 5 minutes (intervalle arrondi au pas cron le plus proche : `7m` devient 6 minutes ;
  `CLAW_LOOP_MAX`, 50 par défaut, limite le nombre de passages pour ménager le quota gratuit).
- `/loop vérifie la CI et traite les commentaires` (sans intervalle) : Claw choisit lui-même
  l'attente après chaque passage, entre 1 minute et 1 heure (outil `ScheduleWakeup`), et arrête
  la boucle quand le travail est fini. Sans nouveau délai, un passage de secours a lieu
  20 minutes plus tard, puis la boucle s'arrête.
- `/loop` seul : le contenu de `.claw/loop.md` ou `.claude/loop.md` (projet), sinon
  `~/.claw/loop.md` ou `~/.claude/loop.md`, sinon une passe d'entretien (travail en cours,
  pull request de la branche, petit nettoyage, rien d'irréversible sans accord).
- `/loop list` montre les tâches ; `/loop stop` arrête les boucles, `/loop stop <id>` une tâche.
- En langage courant : « rappelle-moi dans 45 minutes de pousser la branche », « quelles tâches
  sont planifiées ? », « annule la vérification du déploiement ». Le modèle utilise les outils
  `CronCreate` (expression cron à cinq champs en heure locale, récurrente ou non), `CronList`
  et `CronDelete`.
- Règles : 50 tâches au plus par session ; une tâche récurrente expire au bout de 7 jours ; une
  échéance manquée pendant un long tour passe une seule fois ; les tâches sont enregistrées avec
  la conversation (`<session>.tasks.json`) et reviennent avec `--resume`. La ligne d'état
  indique leur nombre. Une session passée en arrière-plan (`/bg`) emporte ses tâches, qui y
  continuent sans terminal.
- L'heure locale vient du système (`date +%z`, ou PowerShell sous Windows) ; `CLAW_TZ_OFFSET`
  (`+02:00`) la fixe. `CLAW_DISABLE_CRON=1` (ou `CLAUDE_CODE_DISABLE_CRON=1`) coupe tout.
- L'interface web (`claw serve`) fonctionne de la même façon : la page reste libre entre deux
  passages, qui y arrivent comme des tours, et le panneau Agents liste les tâches planifiées.

### Sessions en arrière-plan : `claw agents`

Comme la vue des agents de Claude Code : plusieurs sessions travaillent seules, sans terminal
ouvert, et une seule vue montre lesquelles avancent, lesquelles attendent une réponse et
lesquelles ont fini.

```bash
claw --bg "corrige le test de connexion qui échoue"      # session en arrière-plan
claw --bg --name revue "relis la PR 12 et liste les risques"
claw --bg --exec "cargo test"                           # commande shell en arrière-plan
claw agents                                             # la vue des sessions
```

- **La vue** (`claw agents`) groupe les sessions par état (épinglées, attendent une réponse, en
  cours, terminées) avec leur dernière action ou leur résultat :
  - `↑`/`↓` choisissent une session ; `Espace` ouvre l'aperçu, où une réponse se tape
    directement (`o` ou `n` pour une permission) ;
  - `Entrée` ou `→` ouvrent la session dans ce terminal ; `/exit` y revient ;
  - `Ctrl+X` arrête, et `Ctrl+X` encore la supprime ; `Ctrl+R` renomme ; `Ctrl+T` épingle ;
  - une demande tapée en bas lance une nouvelle session (`!commande` pour un shell).
- **Isolation** : dans un dépôt git, une session lancée en arrière-plan travaille dans son
  propre worktree (`.claw/worktrees/bg-<id>`, branche `claw/bg-<id>`) et y committe son travail.
  `"worktree": {"bgIsolation": "none"}` dans les réglages la fait travailler sur place.
  `claw rm` supprime un worktree resté vide ; celui qui contient des commits est gardé, avec sa
  branche.
- **Permissions et questions** : une session sans terminal ne refuse pas en silence. Elle passe
  dans « attendent une réponse » ; vous répondez depuis l'aperçu, avec `claw reply <id> o`, ou en
  l'ouvrant. Une réponse envoyée pendant qu'elle travaille attend la fin de son tour.
- **Depuis une session** : `/bg [demande]` (ou `/background`) passe la conversation en
  arrière-plan, au même endroit, et libère le terminal ; `/fork [demande]` en lance une copie
  dans son propre worktree pendant que vous continuez. La ligne d'état signale les sessions qui
  attendent une réponse.
- **Depuis l'interface web** : le bouton « Agents » de `claw serve` montre les mêmes sessions,
  en lance de nouvelles, ouvre leurs sorties, répond à leurs questions et permissions, les
  arrête ou les retire ; `/bg` et `/fork` y marchent aussi.
- **Depuis le shell** : `claw attach <id ou nom>` (sorties, puis la conversation ; `Échap` rend la
  main sans l'arrêter), `claw logs <id> [-f]`, `claw reply <id> <texte>`, `claw stop <id>`,
  `claw rm <id>`, `claw agents --json [--all] [--cwd CHEMIN]` pour les scripts (champs `id`,
  `state` : `working`, `blocked`, `done`, `failed`, `stopped`, `waitingFor`…).
- **Sorties** : sous Windows, les lignes écrites par les commandes du système (`ping`, `dir`…)
  dans la page de code de la console (850) sont relues avec leurs accents dans `claw logs`, la
  vue des agents et la page.
- **Fonctionnement** : chaque session est un processus `claw` détaché (pas de service
  permanent), dont l'état vit dans `~/.claw/jobs/<id>/` ; `CLAW_JOB_DIR` lui donne un dossier
  `tmp/`. Sans demande pendant une heure (`CLAW_JOB_IDLE_SECS`), une session non épinglée libère
  son processus ; une réponse la relance depuis sa conversation, qui reste dans `.claw/sessions`
  (`claw --resume`). Sous Windows, les sessions tournent sur une copie de `claw.exe`, pour que
  Claw reste installable pendant qu'elles travaillent.
- **Différences avec Claude Code** : pas de superviseur ni de résumé écrit par un petit modèle
  (la ligne reprend la dernière action ou le début de la réponse) ; l'ouverture d'une session
  arrête son processus d'arrière-plan pendant que vous y travaillez.

### Monitor et messages entre sessions

**Monitor** (outil du modèle, comme dans Claude Code) : Claw lance une commande en arrière-plan
(suivre un journal et filtrer les erreurs, attendre la fin d'une CI, surveiller un dossier) et
chaque nouvelle ligne qu'elle écrit lui revient entre deux tours, sans bloquer la
conversation : il réagit seulement si quelque chose mérite votre attention. Chaque surveillance
a une échéance (5 minutes par défaut, 30 au plus), après laquelle Claw reçoit un avis et peut la
relancer ; `KillShell monitor-1` l'arrête plus tôt, `/tasks` les liste. La commande suit les
règles de `bash` (`Bash(...)` s'applique, le mode auto la juge). Les sessions en arrière-plan
reçoivent aussi leurs événements.

**Messages entre sessions** : les sessions Claw ouvertes sur cette machine (terminal, `-p`,
arrière-plan, interface web) se joignent par leur nom (le dossier, ou celui donné par `/rename`
ou `--name` ; la page de `claw serve` prend le nom du dossier suivi de `-web`).
Le modèle les trouve avec `ListAgents` et leur écrit avec `SendMessage` : une session qui découvre
un changement cassant prévient celle qui en dépend, une longue tâche annonce sa fin,
`notify_when_idle` demande un avis quand l'autre session aura fini son tour.

- Un message est du texte, jamais la conversation ni les fichiers ; il arrive entre deux tours
  (`› Message de @api : …`) et le modèle de l'autre session le lit comme venant d'une autre
  session, pas de vous : il n'approuve rien, ne justifie aucun changement de permissions ou de
  configuration, et ses commandes restent du texte.
- `crossSessionInbound` : `accept`, `hold` (retenus jusqu'à `/peers accept`) ou `refuse`. Sans
  réglage, une session qui demande les permissions accepte, sauf d'une session qui les contourne
  (`danger-full-access`), et inversement. Dans une session en arrière-plan, un message retenu
  la fait passer dans « attendent une réponse » : `o` le délivre, `n` l'écarte.
- `/list-agents` (`/peers`) montre le nom de cette session et les sessions joignables ;
  `/peers accept|drop` traite les messages retenus. `CLAW_DISABLE_PEERS=1` retire la session de
  la messagerie ; une règle `deny` sur `SendMessage` et `ListAgents` empêche d'écrire.
- Les boîtes sont des fichiers dans `~/.claw/peers/<id>/` (cette machine seulement ; pas de
  sessions sur d'autres machines ni dans le cloud).

### Workflows : des agents orchestrés par un script

Comme les workflows dynamiques de Claude Code : pour un travail qui se découpe en nombreuses
parties (audit de chaque route, migration fichier par fichier, recherche recoupée), le modèle
écrit un script qui lance des agents par phases, et Claw l'exécute en arrière-plan. Le résultat,
unique, revient dans la conversation entre deux tours ; la saisie reste libre pendant ce temps.

```javascript
export const meta = {
  name: 'audit-routes',
  description: "Vérifie les contrôles d'accès de chaque route",
  phases: [{ title: 'Inventaire' }, { title: 'Audit' }],
}
phase('Inventaire')
const found = await agent('Liste les fichiers .ts de src/routes.', {
  schema: { type: 'object', required: ['files'], properties: { files: { type: 'array', items: { type: 'string' } } } },
})
phase('Audit')
const audits = await pipeline(found.files, (file) => agent(`Contrôles d'accès manquants dans ${file} ?`, { label: file }))
return audits.filter(Boolean)
```

- **Lancer** :
  - demandez-le : « ultracode », « utilise un workflow », ou le nom d'un workflow enregistré ;
    le modèle appelle l'outil `Workflow`, qu'il n'utilise pas de lui-même ;
  - `/<nom> [arguments]` lance un workflow enregistré dans `.claw/workflows/` ou
    `.claude/workflows/` (projet), `~/.claw/workflows/` ou `~/.claude/workflows/` ; des
    arguments `[…]` ou `{…}` sont lus comme du JSON ;
  - `/deep-research <question>` lance le workflow intégré.
- **API du script** :
  - `agent(demande, options)` rend le rapport de l'agent, la valeur JSON conforme à `schema`, ou
    `null` en cas d'échec. Options : `label`, `phase`, `schema`, `model`, `agentType`,
    `isolation: 'worktree'`.
  - `parallel([...])` lance des fonctions ensemble ; `pipeline(liste, étape1, étape2…)` fait
    passer chaque élément par les étapes.
  - `phase(titre)`, `log(...)` ; `args` contient les arguments.
  - `/workflow-authoring` affiche la référence.
- **Suivre** :
  - `/workflows` liste les exécutions et les workflows enregistrés ;
  - `/workflows <id>` montre les phases, chaque agent (en cours, fini, en échec, repris), le
    journal et le résultat ;
  - `/workflows stop <id>` arrête une exécution ; `/workflows save <id> [nom]` en fait une
    commande ;
  - `/tasks` et le panneau Agents de `claw serve` les montrent aussi.
- **Reprendre** : l'outil `Workflow` avec `resumeFromRunId` relance une exécution ; les agents
  déjà finis avec les mêmes entrées rendent leur résultat gardé, sans requête. Chaque exécution a
  son dossier `.claw/workflows/runs/<id>/` (script, arguments, résultats des agents,
  `result.json`).
- **Sécurité** :
  - le script tourne sous Node.js, dans un contexte `vm` vide, et `node` est lancé avec son
    modèle de permissions : le script ne voit ni fichiers, ni réseau, ni processus ;
  - les agents gardent les permissions de la session et ne peuvent rien demander : une action
    à approuver leur est refusée ;
  - l'outil `Workflow` demande l'autorisation en mode `prompt` et n'est pas permis en
    `read-only`.
- **Modèles gratuits** : chaque agent coûte une dizaine de requêtes.
  - `CLAW_WORKFLOW_MAX_AGENTS` (30 par défaut) limite les agents d'une exécution ;
  - `CLAW_WORKFLOW_MAX_CONCURRENT_AGENTS` (3) limite ceux qui tournent ensemble ;
  - `CLAW_WORKFLOW_SUBAGENT_MODEL` (ou `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL`) impose leur
    modèle.
- **Ailleurs** : en mode `-p`, Claw attend la fin du workflow et passe son résultat comme un tour
  de plus. Une session en arrière-plan reste en vie tant qu'un workflow tourne.
- **Prérequis et désactivation** : Node.js 20 ou plus (`CLAW_WORKFLOW_NODE` pour un autre
  chemin). `CLAW_DISABLE_WORKFLOWS=1`, `CLAUDE_CODE_DISABLE_WORKFLOWS=1` ou
  `"disableWorkflows": true` les coupe.
- **Différences avec Claude Code** : pas de vue plein écran `/workflows` (pause, relance d'un
  agent), mais des commandes texte ; pas d'exécution dans le cloud ; les rapports des agents
  sont dans `.claw/agents/`.

### Channels : des événements poussés dans la session

Comme les channels de Claude Code : un serveur MCP pousse des événements dans la session en
cours (résultat de CI, alerte de supervision, message d'un chat), pour que Claw réagisse là où il
a déjà vos fichiers et le contexte de votre travail. Les événements arrivent entre deux tours ;
la session doit rester ouverte (terminal, `claw serve` ou `--bg`).

```bash
claw --channels server:webhook                 # terminal
claw --channels server:webhook serve           # interface web
claw --channels server:webhook --bg "surveille la CI et corrige ce qui casse"
```

- **Récepteur de webhooks intégré**, sans Node ni Bun : déclarez-le comme serveur MCP, dans
  `.mcp.json` ou `.claw.json` :

  ```json
  { "mcpServers": { "webhook": { "command": "claw", "args": ["channel", "webhook", "--port", "8788"] } } }
  ```

  - Il écoute sur `127.0.0.1` ; chaque `POST` devient un événement, par exemple
    `curl -X POST localhost:8788 -d "build failed on main"`.
  - L'outil `reply` permet au modèle de répondre ; ses réponses sortent sur `GET /events`
    (Server-Sent Events, `curl -N localhost:8788/events`).
  - Avec un jeton (`--token` ou `CLAW_CHANNEL_TOKEN`), chaque requête doit le présenter
    (`Authorization: Bearer …`, en-tête `X-Claw-Token` ou `?token=`).
  - Avec ce jeton, le channel relaie aussi les demandes de permission : elles sortent sur
    `/events` avec un identifiant de cinq lettres, et `POST "yes abcde"` autorise, `"no abcde"`
    refuse. L'en-tête facultatif `X-Claw-Sender` nomme l'expéditeur.
- **Votre propre channel** : tout serveur MCP stdio qui suit le contrat de Claude Code.
  - Il déclare la capacité `experimental["claude/channel"]` et envoie
    `notifications/claude/channel` avec `content` et `meta`.
  - Ses `instructions` sont données au modèle.
  - Avec `claude/channel/permission`, il reçoit
    `notifications/claude/channel/permission_request` et répond par
    `notifications/claude/channel/permission`.
  - Les serveurs écrits pour Claude Code (comme fakechat) marchent tels quels.
- **Ce que voit le modèle** :
  - les événements dans des balises `<channel source="webhook" chat_id="1" …>` ;
  - une note qui les présente comme venant d'un système extérieur, pas de l'utilisateur, et
    qui interdit d'y voir une autorisation ;
  - dans le terminal, une ligne `← webhook : …` par événement.
- **Relais des permissions** : une demande ouverte dans le terminal, la page ou une session en
  arrière-plan part aussi vers les channels qui le déclarent ; la première réponse l'emporte.
  - Le texte relayé est nettoyé (caractères invisibles retirés, 3 500 caractères au plus par
    champ).
  - Les jetons reconnaissables (`sk-…`, `ghp_…`…) y sont masqués.
  - Seul un identifiant émis par Claw et encore ouvert est accepté.
- **Activation et sécurité** :
  - figurer dans `.mcp.json` ne suffit pas : un serveur ne pousse rien sans `--channels` ;
  - `--dangerously-load-development-channels` est accepté comme synonyme, Claw n'ayant pas de
    liste d'autorisation ;
  - `"channelsEnabled": false` dans les réglages bloque tout ;
  - les channels sont ignorés avec `-p` ;
  - `/mcp` liste les channels actifs.
- **Différences avec Claude Code** : seulement `server:<nom>` (les plugins de channel Telegram,
  Discord et iMessage ne sont pas fournis) ; serveurs stdio uniquement.

### Télémétrie OpenTelemetry

Comme le suivi d'usage de Claude Code, Claw peut envoyer ses compteurs et ses événements à
votre collecteur OpenTelemetry (Grafana, Honeycomb, Datadog…), par exemple pour suivre
l'usage d'une équipe. Rien n'est mesuré ni envoyé sans activation explicite :

```bash
export CLAW_ENABLE_TELEMETRY=1            # ou CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer%20votre-jeton"   # si besoin
```

- **Compteurs** (`claw_code.*`) : `session.count` (`start_type` : `fresh`, `resume`,
  `background`, `agents_view`), `token.usage` (`input`, `output`, `cacheRead`,
  `cacheCreation`, par modèle), `cost.usage` (estimation en dollars, nulle pour les modèles
  gratuits), `lines_of_code.count` (`added`, `removed`), `commit.count`,
  `pull_request.count`, `code_edit_tool.decision` (outil, `accept`/`reject`, source,
  langage), `active_time.total` (temps de travail de Claw).
- **Événements** : `user_prompt` (longueur ; le texte seulement avec
  `OTEL_LOG_USER_PROMPTS=1`), `api_request` (modèle, tokens, coût, durée, `main` ou
  `subagent`), `api_error`, `tool_result` (paramètres seulement avec
  `OTEL_LOG_TOOL_DETAILS=1`), `tool_decision`. Chacun porte `session.id`, `prompt.id` et
  `user.id` (identifiant anonyme gardé dans `~/.claw/telemetry-id`).
- **Réglages** : `OTEL_METRICS_EXPORTER` et `OTEL_LOGS_EXPORTER` (`otlp`, `console`, `none`),
  adresses et en-têtes par signal (`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT`…),
  `OTEL_METRIC_EXPORT_INTERVAL` (60 s), `OTEL_LOGS_EXPORT_INTERVAL` (5 s),
  `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=cumulative`, `OTEL_RESOURCE_ATTRIBUTES`
  (`team.id=core,department=r-d`), `OTEL_METRICS_INCLUDE_VERSION=true`.
  `CLAW_OTEL_METRIC_PREFIX=claude_code` reprend les noms de Claude Code, pour réutiliser ses
  tableaux de bord.
- Protocole : `OTEL_EXPORTER_OTLP_PROTOCOL` (ou `OTEL_EXPORTER_OTLP_METRICS_PROTOCOL`,
  `OTEL_EXPORTER_OTLP_LOGS_PROTOCOL`) vaut `http/json` (par défaut), `http/protobuf` ou `grpc`.
  - En HTTP, l'adresse par défaut est `http://localhost:4318` (`/v1/metrics`, `/v1/logs`).
  - En gRPC, c'est `http://localhost:4317`. Une adresse `http://` passe en HTTP/2 sans TLS, comme
    pour un collecteur local, et `https://` passe en HTTP/2 avec TLS.
  - Le protobuf est encodé par Claw lui-même, sans dépendance.
- Ce qui attend part aussi à la fermeture de Claw.
- Un fichier de projet (`.claw/settings.json`) ne peut pas activer la télémétrie ni choisir
  son adresse (`OTEL_*`) : un dépôt cloné ne décide pas où partent vos données d'usage.

### Travail de l'agent

- **Parallélisme** : plusieurs sous-agents, lectures ou recherches demandés dans un même message
  s'exécutent en même temps (les autorisations restent posées une par une).
- **Navigation dans le code** : l'outil `FindSymbol` donne, pour un nom de fonction, méthode,
  classe, type ou constante, sa définition et chacun de ses usages (fichier et ligne) dans tout
  le projet, hors dossiers de construction et dépendances ; plus précis qu'une recherche de texte.
- **`/context`** : ce qui occupe la mémoire du modèle (prompt système, outils, conversation), en
  jetons estimés et en part de sa fenêtre, avec les plus gros morceaux et le seuil de compaction.
- **Tâches de fond** : une commande lancée avec `run_in_background` (serveur de dev, mode watch…)
  garde sa sortie, que l'agent lit avec `BashOutput` et arrête avec `KillShell`. `/bashes` les
  liste (`/bashes kill <id>`) ; elles s'arrêtent quand Claw se termine.
- **Mode plan** : en `read-only`, l'agent soumet son plan avec l'outil `ExitPlanMode` ; si vous
  l'approuvez, la session passe en `workspace-write` et l'agent l'applique dans le même tour.
  Avec `"plannerModel"`, c'est ce modèle qui prépare le plan (voir « Aides au raisonnement »).
- **Reprise** : `claw --continue` (ou `-c`) reprend la dernière session du projet, aussi avec
  une demande ponctuelle (`claw -c prompt "continue"`, comme `claude -c -p`), `claw --resume`
  sans argument propose la liste des sessions récentes. La session est enregistrée à chaque étape
  d'un tour, pas seulement à la fin : un tour interrompu (fenêtre fermée, plantage, coupure) se
  retrouve à la reprise, et un appel d'outil qui était en cours y est noté comme interrompu.
- **Ménage** : `/session rename <id> <titre>` donne un titre à une session (sans titre : retour
  au début de la première demande) ; `/session delete <id>` efface une session enregistrée et
  ses points de restauration ; `/session clear` efface toutes les précédentes (confirmation par
  `/session clear oui`). Les sessions sont dans `.claw/sessions/` du projet, leurs titres dans
  `.claw/session-titles.json`.
- **Quota gratuit** : `/credits` donne aussi le rythme (« quota gratuit épuisé dans environ
  2 h 10 ») et les requêtes de Claw par modèle ; après une réponse, le terminal prévient quand il
  reste moins de 100, 20 puis 0 requêtes gratuites. Le décompte par modèle vient d'un journal
  tenu par Claw (`~/.claw/usage/`, 14 jours), commun au terminal, à l'interface web, aux
  sous-agents et aux mesures.

### Terminal

- Affichage à la manière de Claude Code : encadré d'accueil, saisie entre deux traits avec la
  ligne d'état dessous (mode, modèle, crédits OpenRouter), réponses précédées de `●`, appels
  d'outils `● Commande(cargo test)` avec leur résultat en retrait `⎿` (premières lignes, diff
  numéroté, liste de tâches □ ■ ✓), indicateur animé `✻ Réflexion… (12 s)` pendant le travail et
  `✻ Terminé en 12 s` à la fin du tour. `CLAW_NO_SPINNER=1` retire l'indicateur animé.
- La ligne d'état indique aussi le remplissage du contexte (`contexte 34 %`) : au-delà du seuil,
  la conversation est résumée automatiquement.
- Taper `/` ouvre le menu des commandes avec leur description : flèches pour choisir, `Tab` pour
  compléter, `Entrée` pour lancer, `Échap` pour fermer.
- `Ctrl+O` montre le dernier tour en entier : raisonnement complet et sorties non coupées des
  outils (sous chaque outil, les sorties longues s'arrêtent à « … +N lignes »).
- `Échap` deux fois efface la saisie ; saisie vide, il propose de revenir avant un message
  précédent : la conversation et les fichiers modifiés par Claw reviennent en arrière et le message
  revient dans la saisie pour être corrigé (comme `/undo`).
- Coller un texte de plusieurs lignes le garde en un seul message ; un long collage s'affiche
  `[Texte collé n°1 : 40 lignes]` et part en entier. `Alt+V` joint l'image du presse-papiers
  (`[Image n°1]`).
- Raccourcis de Claude Code dans la saisie :
  - `Ctrl+G` (ou `Ctrl+X Ctrl+E`) ouvre la saisie dans `$VISUAL`, `$EDITOR`, ou à défaut le
    Bloc-notes (Windows) ou `vi` ; le texte enregistré la remplace.
  - `Ctrl+S` met la saisie de côté ; sur une saisie vide, il la reprend.
  - `Ctrl+_` annule la dernière modification de la saisie (mot par mot). Sur un clavier AZERTY,
    c'est `Ctrl+8`, la touche du « _ ».
  - `Ctrl+R` cherche dans l'historique : tapez un extrait, `Ctrl+R` passe à une entrée plus
    ancienne, `Tab` ou `Échap` la garde, `Entrée` l'envoie, `Ctrl+C` annule.
  - `Ctrl+L` efface l'écran et redessine la saisie.
  - Comme dans readline : `Ctrl+A`/`Ctrl+E` (début, fin de ligne), `Ctrl+K`/`Ctrl+U`/`Ctrl+W`
    (supprimer jusqu'à la fin, jusqu'au début, le mot précédent ; `Ctrl+Y` recolle), `Alt+B`/`Alt+F`
    (mot précédent, suivant), `Alt+D` (supprimer le mot), `Ctrl+P`/`Ctrl+N` (historique).
  - Dans une saisie de plusieurs lignes, les flèches haut et bas déplacent d'abord le curseur.
- **Raccourcis personnalisés** : `/keybindings` crée `~/.claw/keybindings.json` (format de Claude
  Code, `~/.claude/keybindings.json` est aussi lu) et liste les raccourcis en vigueur. Chaque bloc
  donne un contexte (`Chat`, `Global`, `Autocomplete`, `HistorySearch`) et une table touches →
  action ; `null` retire un raccourci, les accords s'écrivent `ctrl+x ctrl+e` :

  ```json
  {"bindings": [{"context": "Chat", "bindings": {"ctrl+e": "chat:externalEditor", "ctrl+s": null}}]}
  ```

  Les modifications s'appliquent à la saisie suivante. `Ctrl+C` et `Ctrl+D` ne se changent pas ;
  une action de Claude Code que Claw n'a pas (`chat:fastMode`…) est acceptée et sans effet, une
  faute de frappe est signalée par `/keybindings`.
- `clear` (ou `cls`) efface l'écran, `exit` (ou `quit`) quitte.
- `claw doctor` (ou `/doctor`) vérifie l'installation : copie de `claw` utilisée, configuration,
  modèle et secours, clé et crédits OpenRouter, Ollama, Git, shell, bac à sable, terminal.
- `Échap` (ou `Ctrl+C`) interrompt le tour en cours ; ce que vous tapez pendant un tour est gardé :
  une ligne validée par `Entrée` part juste après, le reste pré-remplit la saisie (Windows ; sous
  Linux et macOS, `Ctrl+C`).
- `!commande` exécute une commande shell directement ; sa sortie accompagne la demande suivante.
- `#note` ajoute une consigne à la section « Notes » de `CLAW.md` (prise en compte aussitôt).
- `@chemin` joint le contenu d'un fichier (ou la liste d'un dossier) à la demande ; `Tab` complète
  les chemins après `@`.
- `/init` crée les fichiers de base puis fait analyser le projet par le modèle pour rédiger
  `CLAW.md` (commandes de build et de test, architecture, conventions).
- Dans l'interface web, un message envoyé pendant un tour part à la fin de celui-ci.

### Rendu plein écran : `/tui fullscreen`

Comme le rendu plein écran de Claude Code : la session s'affiche sur l'écran alternatif du
terminal, comme `vim` ou `htop` (rien à voir avec la taille de la fenêtre).

- **Pourquoi** : pas de scintillement, même dans le terminal de VS Code, tmux ou iTerm2, car seul ce
  qui change est redessiné ; la saisie reste en bas pendant que Claw travaille ; la mémoire reste
  bornée (20 000 lignes d'historique).
- **Activer** :
  - `/tui fullscreen` enregistre le réglage `"tui": "fullscreen"` et relance la conversation en
    plein écran, sans la perdre ;
  - `/tui default` revient au rendu classique, `/tui` dit lequel est actif ;
  - `CLAW_NO_FLICKER=1` (ou `CLAUDE_CODE_NO_FLICKER=1`) démarre en plein écran, et `=0` le
    refuse ;
  - `CLAW_DISABLE_ALTERNATE_SCREEN=1` (ou celle de Claude Code) garde toujours le rendu
    classique, comme une entrée ou une sortie qui n'est pas un terminal.
- **Relire la conversation** :
  - `PgUp`/`PgDn` défilent d'une demi-page, `Ctrl+Origine` remonte au début, `Fin` ou
    `Ctrl+Fin` revient en bas, la molette défile de trois lignes (`CLAW_SCROLL_SPEED`,
    multiplicateur jusqu'à 20) ;
  - tant que vous relisez, la barre du bas compte les nouvelles lignes et la vue ne bouge pas ;
    un clic sur cette barre, `Fin` ou une touche tapée revient en bas.
- **Souris** : la molette passe par Claw, qui capture donc la souris. Selon le terminal,
  `Maj` + glisser garde la sélection native. `CLAW_FULLSCREEN_MOUSE=0` laisse la souris au
  terminal (`PgUp`/`PgDn` restent).
- **Fonctionnement** : la session tourne dans un pseudo-terminal (ConPTY sous Windows). Un
  émulateur VT garde son écran et son historique, et Claw ne dessine que les différences, en
  sortie synchronisée. En quittant, le dernier écran reste affiché dans le terminal.
- **Différences avec Claude Code** :
  - pas de sélection ni de copie dans l'application, ni de clic dans la saisie ;
  - pas de mode transcription avec recherche (`Ctrl+O`, `/`) ;
  - pas de panneau de diff à côté.

### Dictée vocale : `/voice`

Comme la dictée de Claude Code : dans la saisie, parlez au lieu de taper ; le texte s'insère au
curseur, à relire avant l'envoi.

- **Activer** :
  - `/voice` bascule, `/voice hold` (par défaut) ou `/voice tap` choisit le mode, `/voice off`
    coupe ;
  - le réglage enregistré est celui de Claude Code :
    `"voice": { "enabled": true, "mode": "hold", "autoSubmit": false }` (l'ancien
    `voiceEnabled` est lu aussi) ;
  - `CLAW_VOICE=0` coupe la dictée.
- **Dicter** :
  - mode `hold` : maintenez `Espace` pendant que vous parlez, relâchez pour finir (une frappe
    courte reste une espace) ;
  - mode `tap` : `Espace` dans une saisie vide démarre, `Espace` ou `Entrée` arrête ;
  - `Échap` annule ; deux minutes au plus ;
  - `autoSubmit` envoie la demande dès la touche relâchée (mode `hold`).
- **Transcription gratuite** : le premier moteur disponible, dans l'ordre :
  - `CLAW_VOICE_COMMAND`, votre commande, qui reçoit le fichier WAV (`{file}`) et écrit le
    texte ;
  - whisper.cpp en local et hors ligne (`CLAW_WHISPER_MODEL`, et `CLAW_WHISPER_BIN` si
    `whisper-cli` n'est pas dans le `PATH`) ;
  - Groq (`GROQ_API_KEY`, palier gratuit, `whisper-large-v3-turbo`), la meilleure qualité ;
  - sous Windows, la reconnaissance vocale du système, hors ligne et sans clé, utilisée sans
    rien régler ;
  - un modèle audio d'OpenRouter choisi avec `CLAW_VOICE_MODEL` (les modèles gratuits d'OpenRouter
    ne transcrivent pas aujourd'hui) ;
  - OpenAI (`OPENAI_API_KEY`), seulement avec `CLAW_VOICE_ENGINE=openai`.

  `CLAW_VOICE_ENGINE` (`command`, `whisper`, `groq`, `windows`, `openrouter`, `openai`) en
  impose un. La langue est celle du réglage `language` (« french », « fr »), sinon celle du
  système.
- **Micro** : `cpal` sous Windows et macOS, `arecord`, `parecord` ou `ffmpeg` sous Linux. Rien
  n'est enregistré hors d'une dictée, et le son n'est gardé que le temps de la transcription.
- **Vérifier** :
  - `/voice status` montre le micro, le moteur et la langue, sans rien enregistrer ;
  - `/voice test <fichier.wav>` transcrit un fichier ;
  - `CLAW_VOICE_INPUT_FILE=<fichier.wav>` remplace le micro par un fichier (essais).

### Automatisation et éditeurs

- `--output-format stream-json` : un événement JSON par ligne (init, texte, `tool_use`,
  `tool_result`, `result` avec usage et coût). `--input-format stream-json` lit les tours sur
  l'entrée standard (`{"type":"user","message":{"content":"…"}}`), pour piloter Claw depuis un
  script. Sans terminal, une demande d'autorisation est refusée et signalée en JSON.
- `--output-format json` : un seul objet JSON à la fin du tour (réponse, itérations, appels et
  résultats d'outils, usage), avec `notices` : ce que Claw a signalé pendant le tour
  (vérification, relecture faite, approuvée ou en échec, limite de requêtes atteinte...).
- `claw review-replay DEMANDE DIFF RÉPONSE` rejoue la relecture par un autre modèle sur un
  travail enregistré (trois fichiers texte), sans outil : une requête. Le banc d'essais réels
  s'en sert pour comparer deux formulations de la relecture sur le même travail.
- **GitHub** : `/install-github-action` (alias `/install-github-app`) écrit dans le dépôt deux
  workflows qui utilisent un modèle gratuit : `claw-assistant.yml` (un membre du dépôt écrit
  `@claw …` dans une issue ou une PR ; Claw répond et ouvre une PR, ou pousse sur la branche de
  la PR commentée) et `claw-review.yml` (revue en lecture seule de chaque PR, publiée dans un
  commentaire mis à jour à chaque poussée). `/install-github-action assistant` ou `review` pour
  un seul, `--force` pour remplacer les fichiers existants. L'action `.github/actions/claw` est
  décrite dans [`docs/github-action.md`](docs/github-action.md).
- **VS Code** : l'extension `editors/vscode` (paquet `claw-code-0.3.0.vsix`, à installer avec
  « Extensions : installer depuis VSIX… ») ouvre l'agent dans un panneau (`Ctrl+Alt+K`), envoie la
  sélection (`Ctrl+Alt+L`) ou le fichier courant.
  - **Diff de l'éditeur** : quand l'agent demande l'autorisation d'écrire ou de modifier un
    fichier, le changement s'ouvre dans un diff de VS Code (fichier actuel à gauche, proposition
    à droite), avec **Accepter** et **Refuser** dans la barre de titre du diff et dans la barre
    d'état (`Ctrl+Entrée`, `Ctrl+Maj+Retour arrière`). Rien n'est écrit avant l'accord ; un refus
    peut porter une consigne pour l'agent. La réponse peut aussi se donner dans le panneau.
  - **Problèmes de l'éditeur** : l'agent lit les erreurs que VS Code souligne (voir « Erreurs de
    l'éditeur » dans les aides au raisonnement).
  - **Fichiers modifiés** : après un tour qui a modifié des fichiers, un bouton de la barre d'état
    ouvre le diff « avant Claw / maintenant » de chacun (aussi pour toute la session).
  - Après une mise à jour de `claw` (`cargo install`), le panneau **se recharge seul** sur la
    même session, à la fin du tour en cours ; sous Windows il lance une copie de `claw.exe`, pour
    que le fichier installé ne soit pas verrouillé pendant la mise à jour.

  Voir `editors/vscode/README.md`.
- **Éditeurs** : `claw acp` fait de Claw un agent de l'Agent Client Protocol (Zed, Neovim avec
  CodeCompanion, Emacs…). Dans Zed : `"agent_servers": { "Claw": { "command": "claw", "args": ["acp"] } }`.
  Les autorisations s'affichent dans l'éditeur, les modes de permission y sont sélectionnables.
- **Recherche web** : `CLAW_SEARXNG_URL` (SearXNG, libre et auto-hébergeable, format json activé),
  `BRAVE_API_KEY` ou `TAVILY_API_KEY` ; à défaut, la page de résultats de DuckDuckGo.
- **Lecture d'une page web** (outil `WebFetch`) : le HTML devient un texte lisible (titres, listes,
  code, liens), sans scripts, styles ni menus ; quand la page déclare son contenu principal
  (`<main>`), seul celui-ci est lu. Une page de moins de 12 000 caractères est rendue entière.
  Au-delà, Claw rend son début et les passages qui contiennent les mots de la question, sans les
  adresses des liens, et le modèle lit la suite en donnant une position (`offset`). Aucun modèle
  n'est appelé pour résumer : lire une page ne coûte pas de requête en plus. Limites : une page
  construite par JavaScript dans le navigateur arrive vide (`WebFetch` n'exécute pas de
  JavaScript), et les PDF et les images ne sont pas lus.
- **Voir une page comme dans le navigateur** (outil `ViewPage`) : un modèle ne voit pas le
  navigateur. Il prenait une réponse 200 pour une page qui marche, supposait la cause d'un défaut
  d'affichage, et vous renvoyait vers la touche F12. `ViewPage` ouvre l'adresse dans Edge, Chrome
  ou Chromium, sans fenêtre et avec un profil temporaire, puis rend le texte affiché une fois le
  JavaScript exécuté (un badge « Hors ligne », un message d'erreur, une zone restée vide) et ce
  que la page a écrit dans la console : erreurs, exceptions avec leur fichier et leur ligne,
  requêtes en échec, refus CORS. Pour un projet local, le modèle lance d'abord le serveur en
  arrière-plan puis donne son adresse `http://localhost` ; une page sans serveur s'ouvre par
  l'adresse `file:///` d'un fichier `.html` du projet (sous Windows, la forme de Git Bash,
  `file:///tmp/...` ou `file:///c/Users/...`, est traduite comme les chemins des fichiers ; jamais d'un fichier hors du dossier de
  travail). Limites : l'outil montre la page telle
  qu'elle se charge, sans cliquer ni saisir, et ne rend pas d'image. Il n'est proposé au modèle
  que si un navigateur est installé ; `CLAW_BROWSER` désigne l'exécutable à utiliser.
- **Icônes** (outil `Icon`) : 1 539 icônes au trait de [Lucide](https://lucide.dev) (licence
  ISC, jointe dans `crates/runtime/src/icons/LUCIDE_LICENSE`), rendues en `<svg>` complet prêt à
  coller : taille, couleur du texte (`currentColor`), `aria-hidden`, ou `role="img"` et
  `aria-label` quand l'icône porte un sens. Plusieurs noms en un appel (`map-pin`, `phone`,
  `clock`, `croissant`) ; une recherche par mots anglais, avec des équivalents français
  (« téléphone », « adresse », « pain ») ; un nom inconnu reçoit les plus proches. Un petit
  modèle dessine mal en SVG : ses icônes faites main valaient à la page une finition de 5/10.
  Aucun emoji dans une interface : ces icônes sont du SVG.
- **Icônes jamais dessinées à la main** : chaque petite image vectorielle d'une page (un `<svg>`
  ou un `<symbol>` à la taille d'une icône, y compris dans un script ou une adresse `data:` d'une
  feuille de style) est comparée aux icônes de Lucide et de [Feather](https://feathericons.com)
  (licence MIT, jointe dans `crates/runtime/src/icons/FEATHER_LICENSE`), l'ancêtre de Lucide que
  les modèles recopient de mémoire. La comparaison porte sur la géométrie des formes, ramenée en
  coordonnées absolues : une ligne vaut le chemin qui passe par les mêmes points, l'ordre des
  formes ne compte pas, une ancienne version à un point près reste une icône du jeu. Une icône
  qui n'est dans aucun des deux est dessinée à la main : Claw le dit au modèle dès l'écriture du
  fichier, avec sa ligne et les noms à demander à l'outil `Icon` (une classe `icon-phone` donne
  `phone`), et `Screenshot` la relève comme un défaut. Sur les pages des essais précédents,
  seules de vraies icônes faites main ou mal formées restent signalées (un lotus en 48 x 48, une
  étoile bancale, un bonhomme bâton) ; les logos et ornements nommés comme tels sont laissés.
- **Composants** (outil `Component`) : 19 composants interactifs qui marchent dès qu'ils sont
  collés (HTML, CSS aux variables du socle de la méthode, JavaScript sans dépendance),
  accessibles, animés avec mesure et arrêtés par `prefers-reduced-motion`, leurs icônes prises
  dans Lucide : `nav-menu` (en-tête et menu du téléphone), `accordion` (questions),
  `tabs`, `dialog` (fenêtre modale), `toast` (messages de confirmation), `carousel`,
  `booking` (jours des deux semaines à venir, créneaux, places restantes, fichier `.ics` pour
  l'agenda), `filter` (étiquettes et recherche, état dans l'adresse), `form` (erreurs après la
  saisie, brouillon gardé, envoi à un service de formulaires ou par e-mail prérempli), `cart`
  (panier ou devis), `theme-toggle` (mode sombre), `copy`, `share`, `open-now` (« Ouvert
  maintenant · ferme à 19 h »), `directions` (itinéraire, appel, carte OpenStreetMap), `reveal`
  (apparition au défilement), `count-up` (chiffres clés), `scroll-effects` (progression de
  lecture, parallaxe en CSS seul), `page-transitions` (fondu entre les pages). Plusieurs noms
  en un appel, en français aussi (« panier », « réservation », « horaires ») ; les composants
  dont l'un se sert (le panier se sert de `dialog` et de `toast`) viennent avec lui. Sans
  serveur, rien n'est perdu : un formulaire part en e-mail prérempli, une réservation reste dans
  le navigateur, et `data-endpoint` les envoie à un service de formulaires. Tous sont essayés
  ensemble dans un vrai navigateur, au téléphone et à l'ordinateur : aucun bouton mort, aucune
  erreur, aucun bloc invisible.
- **Ornements** (outil `Ornament`) : des formes décoratives SVG tracées par formules aux
  couleurs du plan, nettes à toute taille : `blob` (galets superposés derrière un titre ou une
  image), `waves` (vagues en couches sur toute la largeur, au bas d'un hero), `dots` et `lines`
  (textures discrètes), `topography` (courbes de niveau), `grain` (grain sur un aplat), `arch`
  (une à trois arches pour cadrer une image ou un texte), `frame` (médaillon autour d'un avatar
  ou d'une initiale), `sprig` (rameau), `rays` (soleil). Chacune vient en `<svg>` complet avec
  `aria-hidden`, et en adresse `data:` pour un fond CSS ; une autre `seed` donne une autre forme
  de la même sorte, et seules des couleurs en hexadécimal entrent dans le SVG. Chaque sorte vient
  avec son CSS de placement exact (les vagues en bas de section, en position absolue et pleine
  largeur ; un galet à demi caché derrière un titre ou une photo) : vu dans un essai réel, des
  vagues avaient été posées dans une colonne de la grille du hero, au milieu de la photo.
- **Photos libres** (outil `Photo`) : voir « Photos libres » dans les aides (`CLAW_PHOTOS=0`
  retire l'outil).
- **Captures de la page** (outil `Screenshot`) : le même navigateur prend des captures au
  téléphone et à l'ordinateur (ou `tablet`, ou `LARGEURxHAUTEUR` ; la page entière avec
  `full_page`, un thème clair ou sombre), après des `actions` qui amènent la page dans l'état à
  vérifier (cliquer, saisir, faire défiler). Les captures suivent le résultat en images ; un
  modèle qui ne lit pas les images en reçoit la description (voir « Lecteur d'images »). Chaque
  capture est aussi gardée en JPEG (`file`), avec un aperçu de son premier écran de 160 pixels de
  large (`preview`, quelques kilo-octets) : Claw Web en fait une vignette dans le journal du
  passage, et montre la capture entière d'un toucher. Chaque
  capture porte aussi les défauts mesurés dans la page, en phrases qui disent quoi corriger :
  contenu plus large que l'écran, avec sa cause (largeur calculée trop grande, rangée flex qui
  ne passe pas à la ligne, colonnes de grille trop larges, élément de grille ou de flex resté à
  `min-width: auto`, texte sur une ligne, image, tableau, champ ou mot trop long ; et un
  `overflow-x: hidden` sur `body` signalé comme cachant le symptôme : vu dans un essai réel, un
  modèle a cherché six fois sans trouver, alors qu'une ligne `min-width: 0` suffisait), textes ou boutons coupés et superposés, images cassées, texte
  trop petit ou trop pâle, en-tête fixe qui couvre le titre ou une navigation qui prend plus du
  quart de l'écran d'un téléphone, liens morts (`href="#"`, ancre vers une section absente),
  champ sans étiquette (ou dont le `label for=` vise un autre élément de même id), id employé
  plusieurs fois (un lien `#id`, un `label for=` et un script n'atteignent que le premier : vu
  dans un essai réel, `section#cours` et `select#cours`), champ rouge avant toute saisie,
  bouton à la police du navigateur, image
  sans `alt`, bouton sans nom, texte de remplissage (lorem ipsum, « votre texte ici »), police
  Times par défaut, focus invisible au clavier (mesuré page focalisée et transitions coupées :
  sans cela, un focus bien stylé passait pour invisible), texte collé au bord de l'écran d'un
  téléphone, icône dessinée en géant (un `<svg>` sans taille remplit son conteneur : vu dans un
  essai réel, cinq étoiles d'avis larges comme la page), emoji dans la page (seul, en icône,
  ou dans une phrase : un emoji est dessiné par la police de l'appareil, différent sur chaque
  téléphone et ordinateur, et n'appartient à aucun design ; chaque icône et image se dessine en
  SVG inline), section qui n'affiche que son titre (du texte caché sur cet écran par une media
  query qui masque une version sans montrer l'autre, ou une liste, un tableau, un bloc à id
  restés vides : vu dans un essai réel, le planning d'un studio écrit pour le téléphone et vide
  sur ordinateur), polices déclarées en CSS mais jamais chargées (avec la cause quand c'est le
  lien Google Fonts). Chaque défaut et chaque remarque de design donnent aussi **les règles CSS
  qui dessinent les éléments en cause, avec leur fichier et leur ligne** (« (CSS rules:
  styles.css:761 .booking-content; styles.css:784 .form-grid) », lues dans le navigateur) : le
  modèle va droit à la règle au lieu de lire le fichier par morceaux. Vu dans un essai réel : sur
  environ 110 appels d'un tour, 58 étaient des lectures et des recherches dans un CSS de 1 000
  lignes. Le texte réservé aux lecteurs d'écran (`.visually-hidden`) et les boutons sans
  texte (icône du menu) ne sont pas pris pour des défauts. Au téléphone, Claw essaie le menu : des liens de
  navigation cachés sans bouton pour les ouvrir, ou un bouton qui ne les montre pas après un
  clic, sont signalés. À part, des remarques de design (`designNotes`) nomment ce qui donne un
  air générique ou peu fini sans être une erreur : polices de tous les jours, lignes de plus de
  75 caractères, corps de texte sous 16px ou interligne sous 1,4, h1 à peine plus grand que le
  texte, longs paragraphes centrés, deux étiquettes en capitales ou plus, trait décoratif sous
  les titres, cartes identiques sans image, une même photo utilisée plusieurs fois (image ou
  fond CSS : vu dans un essai, une photo de cours en fond du hero, dans la section du studio et
  à la place des portraits), sections aux marges
  verticales toutes différentes, couleur saturée sur plus de 15 % de la page, liens et boutons
  qui ne changent pas au survol (`:hover` forcé sur chacun par le protocole du navigateur, et
  son apparence comparée : le script d'une page ouverte en `file://` ne peut pas lire les
  règles de ses feuilles, et l'ancienne mesure y voyait à tort un site sans survol, ce qui a
  fait ignorer au modèle les vrais défauts), cibles tactiles sous 44px au téléphone, bandes
  vides de plus d'un demi-écran sans rien à lire ni à voir, animations sans
  `prefers-reduced-motion`, liens, boutons ou cartes qui animent leur taille ou leur position
  (une transition sur `width`, `margin` ou `top` saccade : `transform` et `opacity` restent
  fluides), retours de survol de plus de 0,6 s, animations qui tournent en boucle. En revue
  visuelle, le modèle les corrige, et n'en garde une que si la demande l'exige.
  Avant de mesurer et de capturer, `Screenshot` fait défiler toute la page puis revient en haut :
  les apparitions au défilement et les images différées se déclenchent, comme pour un visiteur
  (une capture pleine page montrait sinon des sections vides), et un bloc resté transparent
  après ce défilement est un défaut. Après la capture, à l'ordinateur, il **clique chaque bouton**
  de la page et relève ceux qui ne font rien (ni changement de la page, ni défilement, ni
  message, ni envoi, ni copie, ni téléchargement) et ceux qui provoquent une erreur, avec le
  message : « clicking button#broken "Envoyer": ReferenceError: missingFunction is not
  defined ». Rien ne quitte la page pendant l'essai : une navigation est retenue et comptée
  comme une action. Un bouton qui ne fait rien est le défaut le plus fréquent d'un site généré.
  L'onglet ou l'étape déjà choisis (`aria-selected`, `aria-current`) ne sont pas essayés, un
  indicateur de chargement ne compte pas comme animation en boucle, et un lien d'évitement qui
  glisse depuis le haut au focus n'est pas une transition sur la mise en page : trois faux
  positifs vus dans un essai réel.

### Bac à sable des commandes

Sous Linux, les commandes peuvent être isolées par namespaces (`unshare`). Sur tous les systèmes
(Windows compris), `"sandbox": { "backend": "docker", "image": "docker.io/library/rust:1", "networkIsolation": true }`
(ou `CLAW_SANDBOX=docker|podman`, `CLAW_SANDBOX_IMAGE`) exécute chaque commande shell dans un
conteneur qui ne voit que le dossier du projet (`/workspace`), sans réseau si demandé ; les
conteneurs sont arrêtés à l'interruption, au délai dépassé et à la fin de Claw.

- Image par défaut : `docker.io/library/debian:stable-slim`. Donnez des noms complets
  (`docker.io/library/…`) : Podman refuse souvent les noms courts.
- `allowedMounts` ajoute des dossiers en lecture seule (`D:\donnees` devient `/mnt/d/donnees`).
  L'option `isolateNetwork` d'une commande prime sur `networkIsolation`.
- Sous Linux, les fichiers créés dans le projet appartiennent à l'utilisateur (Docker reçoit son
  uid:gid, Podman sans privilèges garde son identité), pas à root.
- Testé en CI avec Docker et Podman : `CLAW_TEST_CONTAINER=docker cargo test -p runtime --test
  container_sandbox -- --ignored --test-threads=1`.

### Langue des réponses

`"language": "français"` dans la config (par exemple `~/.claw/settings.json`) fixe la langue des
réponses, y compris pour les commandes `/review`, `/audit`… Sans réglage, l'agent répond dans la
langue où vous lui écrivez. Les notes que Claw adresse au modèle après un contrôle (vérification,
exigences, relecture) sont en anglais : chacune rappelle donc de répondre dans votre langue, pour
que la réponse finale ne bascule pas en anglais.

**Ni tiret cadratin ni emoji en sortie.** Claw n'écrit jamais le tiret long (U+2014) : il met
« - » à la place. Il n'écrit jamais d'emoji non plus : une coche ou une croix dessinée devient
« ✓ » ou « ✗ », une flèche dessinée devient « → », les autres pictogrammes (fusée, dossier,
panneau d'attention, drapeau…) sont retirés avec l'espace qui les suivait. Le prompt système le
demande au modèle (réponses, code, commentaires, documents, messages de commit), et un filtre
l'applique de toute façon à tout ce qu'un modèle écrit en sortie : réponses, réflexion affichée,
sous-agents, avis d'un autre modèle, résumés de conversation. Sont des emoji tous les caractères
qu'Unicode classe `Extended_Pictographic` ou `Emoji_Presentation`. Les fichiers que l'agent lit
ou écrit ne passent pas par ce filtre : pour eux, seule la consigne s'applique (demandez
explicitement un emoji si votre page doit en contenir). Un test échoue si un tiret cadratin ou
un emoji revient dans les sources du projet.

La connexion OAuth est aussi disponible :

```bash
cargo run --bin claw -- login
```

### Installation locale

```bash
cargo install --path crates/claw-cli --locked
```

Sous Windows, pour réinstaller pendant que `claw` tourne (panneau de VS Code ouvert) :

```powershell
.\scripts\install.ps1
```

Un programme en cours d'exécution verrouille son fichier : le script met l'ancien `claw.exe` de
côté, installe, puis efface les anciennes copies. Si la compilation échoue ou est interrompue,
l'ancienne version est remise en place.

### Construction depuis les sources

```bash
cargo build --release -p claw-cli
```

Le binaire est `target/release/claw.exe` sous Windows (`target/release/claw` ailleurs).
Sous Windows, Git for Windows est recommandé : son `bash` sert aux commandes shell de l'agent,
aux hooks et aux scripts des plugins (`CLAW_SHELL` permet d'en choisir un autre).

### Binaires et intégration continue

Le workflow `.github/workflows/rust.yml` (à la racine du dépôt) vérifie le formatage et lance
les tests sous Windows, Linux et macOS à chaque push ou pull request sur `main`. Pousser une
étiquette `v*` (par exemple `git tag v0.2.0 && git push --tags`) publie une release GitHub
avec les binaires Windows (x86_64), Linux (x86_64) et macOS (Apple Silicon).

### Exécution

Depuis l'espace de travail :

```bash
cargo run --bin claw -- --help
cargo run --bin claw --
cargo run --bin claw -- prompt "résume cet espace de travail"
cargo run --bin claw -- --model sonnet "passe en revue les derniers changements"
```

Depuis la version release :

```bash
./target/release/claw
./target/release/claw prompt "explique crates/runtime"
```

## Capacités prises en charge

- REPL interactif et exécution de prompts ponctuels
- Inspection des sessions sauvegardées et flux de reprise
- Outils intégrés conscients de l'espace de travail pour le shell, la lecture/écriture/édition de fichiers, la recherche, la récupération/recherche web, les todos et les mises à jour de notebook
- Commandes slash pour le statut, le compactage, l'inspection de config, le diff, l'export, la gestion de session et le rapport de version
- Découverte d'agents et de skills locaux avec `claw agents list` et `claw skills`
- Découverte et gestion de plugins via la CLI et les surfaces de commandes slash
- Connexion/déconnexion OAuth ainsi que sélection de modèle/fournisseur depuis la ligne de commande
- Chargement d'instructions/config conscient de l'espace de travail (`CLAW.md`, fichiers de config, permissions, paramètres de plugins)

## Limitations actuelles

- Pas de publication sur crates.io : installation par `cargo install --path` ou par les binaires des releases GitHub
- Les binaires ne sont pas signés (Windows SmartScreen peut afficher un avertissement au premier lancement)
- Certaines couvertures d'intégration avec un fournisseur en direct sont en opt-in car elles requièrent des identifiants externes et un accès réseau
- La surface des commandes peut continuer à évoluer pendant la série `0.x`

## Implémentation

L'espace de travail Rust est l'implémentation produit active. Il comprend actuellement ces crates :

- `claw-cli` - binaire orienté utilisateur
- `api` - clients de fournisseurs et streaming
- `runtime` - sessions, config, permissions, prompts et boucle d'exécution
- `tools` - implémentations des outils intégrés
- `commands` - registre et handlers des commandes slash
- `plugins` - découverte, registre et support de cycle de vie des plugins
- `lsp` - types de support du protocole de langage et helpers de processus
- `server` et `compat-harness` - services de support et outillage de compatibilité

## Feuille de route

- Publier des artefacts de release empaquetés pour les installations publiques
- Ajouter un workflow de release reproductible et une discipline de changelog pérenne
- Étendre la vérification de plateforme au-delà de la matrice CI actuelle
- Ajouter plus d'exemples orientés tâches et de documentation pour les opérateurs
- Continuer à resserrer la couverture des fonctionnalités et à peaufiner l'UX dans toute l'implémentation Rust

## Notes de version

- Brouillon des notes de version 0.1.0 : [`docs/releases/0.1.0.md`](docs/releases/0.1.0.md)

## Licence

Voir la racine du dépôt pour les détails de licence.
