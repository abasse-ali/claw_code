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
- diagrammes Mermaid à la demande ; commandes `/review`, `/audit`, `/archi`, `/adr`, `/fix`,
  `/models`, `/credits`, `/mcp`, `/cost`, `/compact`…

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
  (`gemini:`, `groq:`), chacun avec sa clé. Le plan gratuit de Mistral ne permet plus de créer
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
- **Choisir un modèle Gemini** : `/models gemini` liste les modèles de l'offre gratuite de Google
  que votre clé ouvre (une simple lecture du catalogue de Google, aucune génération), et la page
  les montre dans la catégorie **Google Gemini** du menu des modèles. Gratuits d'après la page
  des tarifs de Google (relevée le 10 octobre 2026) : `gemini-3.8-flash`, `gemini-3.6-flash`,
  `gemini-3.5-flash-lite`, `gemini-3.1-flash-lite`, `gemini-2.5-pro`, `gemini-2.5-flash`,
  `gemini-2.5-flash-lite`. `gemini-3.1-pro-preview` est payant et n'est pas proposé. Exemple :
  `/model gemini:gemini-3.8-flash`.
- La limite de jetons de sortie suit le catalogue (sortie maximale et contexte du modèle) ;
  `/cost` indique le coût estimé (0 $ pour un modèle gratuit).
- Chaque étape de l'agent consomme une requête du quota gratuit ; en cas de limite atteinte
  (erreur 429), la session continue et un conseil s'affiche.

### Permissions

| Mode (`--permission-mode`, `/permissions`) | Alias Claude Code | Comportement |
|---|---|---|
| `prompt` (défaut) | `default` | Lecture libre ; demande avant chaque modification ou commande |
| `read-only` | `plan` | Mode plan : lecture et commandes sans effet uniquement ; l'agent propose un plan |
| `workspace-write` | `acceptEdits` | Modifie les fichiers sans demander ; demande avant les commandes |
| `danger-full-access` | `bypassPermissions` | Tout est autorisé sans demande |

Les commandes en lecture seule (`git status`, `git diff`, `ls`, `cat`…) ne déclenchent pas de demande.
Dans le terminal, `Maj+Tab` passe d'un mode à l'autre (demander → éditions acceptées → plan) ; le
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
    "ask": ["Bash(git commit:*)"]
  }
}
```

Une commande composée (`a && b`, `a | b`, `a; b`) n'est autorisée par une règle que si chacune
de ses parties l'est ; une substitution (`$(…)`, backticks) déclenche toujours une demande.

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
| `UserPromptSubmit` | à chaque demande ; stdout ajouté au contexte | rejette la demande |
| `Stop` / `SubagentStop` | fin de tour de l'agent / d'un sous-agent | relance l'agent avec la raison (5 fois max) |
| `SessionStart` / `SessionEnd` | début (`startup`, `resume`, `clear`) / fin de session | - |
| `PreCompact` | avant une compaction (`auto`, `manual`) | - |
| `Notification` | quand une autorisation est demandée (en arrière-plan) | - |

Chaque hook reçoit sur stdin un JSON (`hook_event_name`, `session_id`, `cwd`, champs de
l'événement). La raison d'un blocage est lue sur stderr (sinon stdout). La sortie JSON de
Claude Code est comprise (`decision: "block"`, `hookSpecificOutput.additionalContext`…). Délai
par défaut : 60 s. Les hooks des plugins sont aussi appliqués. `/hooks` affiche la configuration.

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
  `gemini:`, `cerebras:`, `together:`, `fireworks:` (clé `<NOM>_API_KEY`). Adresse modifiable par
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
  n'est exécuté et le modèle est invité à envoyer un appel plus petit. Dans tous les cas, un
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
  retenu d'un lancement à l'autre (`~/.claw/model-cooldowns.json`). Un modèle en échec est mis de côté un
  moment ; `CLAW_FALLBACK=0` désactive la bascule.
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
| Vérification imposée | Quand le modèle dit avoir fini après avoir modifié des fichiers, Claw lance la vérification du projet (`cargo check`, `go build`, `tsc`, tests Python… ou `"verify"` ; des tests Python écrits pour pytest, qui l'importent ou ne sont que des fonctions `test_...`, sont lancés avec pytest s'il est installé, même sans fichier de réglage). En cas d'échec, la sortie repart au modèle, qui corrige (3 corrections au plus) ; si cela échoue encore, Claw l'annonce : « le travail n'est pas validé » | `CLAW_VERIFY=0` |
| Syntaxe immédiate | Après chaque modification, contrôle de syntaxe du fichier (JSON, Python, JavaScript, Rust, Go, shell) ; l'erreur est jointe au résultat de l'outil | `CLAW_DIAGNOSTICS=0` |
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
| Escalade | Au deuxième échec de la vérification, ou au deuxième recadrage, Claw vous propose de confier la suite à un autre modèle (le gratuit le plus puissant en premier) ; sans réponse en 60 s, le même modèle continue. Avec `"modelSwitch": "auto"` ou sans personne pour répondre, **le modèle gratuit le plus puissant du moment** reprend la fin du tour ; le tour suivant repart avec le modèle habituel. Sans réglage à faire, et jamais vers un modèle payant | `CLAW_ESCALATION=0` |
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
| Petit projet | Au premier tour d'une session, si le dépôt Git du projet est petit (20 fichiers au plus, 24 000 caractères en tout, chaque fichier de la taille d'une vue), Claw joint tous ses fichiers au message, en vues comme après une modification : le modèle n'a pas à les lire un par un (un petit modèle y passait sept requêtes avant d'écrire quoi que ce soit). Chaque vue est remplacée quand son fichier change ; lire un fichier qu'une vue à jour montre déjà est signalé au modèle | `CLAW_SMALL_PROJECT=0` |
| Modifications perdues | Une commande du modèle supprime un fichier qu'il avait modifié pendant le tour, ou le ramène à son état du début du tour (`git checkout`, `git restore`, `git stash`) : Claw garde sa dernière version (dans `.claw/kept/`), le dit au modèle avec la commande qui la remet en place, et vous prévient. Il ne la remet pas lui-même : une suppression peut être voulue. Un fichier créé puis effacé dans le même tour (un script d'essai) ne compte pas. Vu dans un essai réel : le petit modèle a supprimé `conges.py` par une commande, l'a restauré avec `git checkout` et a refait tout son travail, deux fois | `CLAW_LOST_EDITS=0` |
| Fichier montré après modification | Le résultat d'une modification porte le fichier tel qu'il est maintenant (en entier s'il est petit, sinon les lignes autour du changement) : le modèle n'a plus à le relire avant la modification suivante, ce qui lui coûtait une requête à chaque fois, et la conversation ne garde qu'une vue à jour par fichier | `CLAW_FILE_VIEW=0` |
| Essais regroupés | Trois réponses de suite faites d'une seule lecture ou d'une seule commande (même refusée par le programme essayé) : Claw demande de regrouper ce qui est déjà prévu (plusieurs appels dans une réponse, commandes enchaînées), chaque réponse coûtant une requête du quota, et montre les trois dernières commandes réunies en un seul appel. Redit à la sixième et à la neuvième réponse isolée, trois fois au plus par tour. De même quand une réponse ne fait que mettre à jour la liste de tâches alors que du travail suit : la mise à jour part avec l'appel suivant | `CLAW_BATCH_CALLS=0` |
| Compte rendu gardé | Après un contrôle de Claw qui n'a rien fait changer, le modèle répond parfois en quelques lignes (« la relecture confirme... ») au lieu de redonner son compte rendu. Quand cette réponse fait moins du tiers de la précédente, Claw remet le compte rendu devant elle : le dernier message, le seul que vous lisez, reste complet, et la page rouvre la réponse qu'elle avait repliée | `CLAW_KEPT_ANSWER=0` |
| Relecture inutile | Le modèle relit un fichier qui n'a pas changé depuis sa lecture précédente (souvent juste avant de le modifier, par habitude) : la conversation ne garde que la copie la plus récente, et quand les deux lectures sont du même tour, Claw lui dit que celle-ci ne lui a rien appris pour le prix d'une requête, deux fois au plus par tour. Vu dans un essai réel : quatre requêtes sur les dix premières | `CLAW_REPEATED_READS=0` |
| Cas limites | Quand votre demande nomme un genre de fonctionnalité (recherche ou filtre, montants, dates, CSV ou Excel, pagination, formulaires, envois de fichiers, comptes, tris, soldes et réservations, doublons, adresses e-mail et téléphones, bases SQL, clés d'API, OCR et traduction, images, appels à un service extérieur, liaison entre front et back, API JSON, code d'une page web, données gardées en JSON, fichiers texte sous Windows, scripts en ligne de commande), Claw joint pour le modèle, dès le départ, ce qu'un ingénieur expérimenté vérifie d'office pour ce genre-là (pour une recherche : accents et majuscules, « oeufs » pour « Œufs », recherche vide, plusieurs mots, le `+` d'une URL ; pour des dates : « 03/04 » est le 3 avril, une fin avant le début est refusée, le dernier jour compte), puis le rappelle, chaque point à prouver, au contrôle des exigences ou à la demande de preuves d'un problème signalé, et le relecteur le confronte au code. Chaque rappel nomme le piège précis (par exemple : `replace(" ", "")` ne retire pas l'espace insécable qu'écrit Excel) : un rappel général ne changeait rien. Les faits cités ont été vérifiés sur une vraie machine (Python 3.13 sous Windows, Pillow, OpenCV, Node), et les mots qui reconnaissent un genre évitent les faux amis (« sortie », « format », « Excellent », « erreur de connexion »). Chaque essai porte le résultat attendu : sans lui, un modèle gratuit faisait les essais qui confirmaient son code. Et une ligne qui s'applique fait partie de la demande, même si vous ne l'avez pas écrite : sans cette phrase, un modèle a lu la ligne des doublons puis l'a écartée parce que la demande « ne le dit pas explicitement ». Mesuré sur des essais réels : 9/12 devient 12/12 (recherche), 3/4 devient 4/4 (CSV), 10/12 devient 12/12 (dates et soldes), 11/12 sans l'aide devient 12/12 (doublons) | `CLAW_EDGE_CASES=0` |

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
  "ultracode": true,
  "turnRequestLimit": 100
}
```

- `"verify"` : une commande, une liste de commandes, ou `false`. Sans réglage, Claw la déduit du
  projet. Dans une session non interactive, la commande doit être autorisée par une règle
  `permissions.allow` ; sinon Claw annonce « Vérification non lancée […] : le travail n'est pas
  vérifié ».
- `"advisorModels"` : modèles consultés pour la relecture, le conseil et le second avis, du
  préféré au moins préféré. Sans réglage et avec une clé OpenRouter : les meilleurs modèles
  gratuits autres que celui de la session (`claw doctor` les affiche, ligne « Autres avis »). Un
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

**Compétences (skills)** : un dossier `<nom>/SKILL.md` placé dans `.claude/skills/` (ou
`.claw/skills/`, `.codex/skills/`) du projet, d'un dossier parent ou du dossier personnel. Claw
donne au modèle le nom et la description de chacune (40 au plus) ; quand une demande y
correspond, le modèle charge ses instructions avec l'outil `Skill` et lit les fichiers qui
l'accompagnent. `claw skills` (ou `/skills`) les liste. Une compétence marquée
`disable-model-invocation: true` ne se lance que par `/nom`.

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
  identifiant de modèle, ou `"off"` pour ne rien faire décrire.
- **Raisonnement** : sans réglage, un modèle d'OpenRouter qui sait raisonner reçoit le plus haut
  niveau de réflexion qu'il accepte (`max`, ou `high` s'il raisonne par budget) : ses réponses
  sont plus lentes, mais plus réfléchies. `/think off|low|medium|high|max` (`max` : le plus haut
  niveau que le modèle accepte ; ou `"reasoning"` dans la config,
  `CLAW_REASONING`) règle l'effort de réflexion des modèles qui raisonnent (DeepSeek-R1, Qwen3,
  GLM, série o d'OpenAI…) et des modèles Claude via l'API Anthropic (raisonnement étendu, blocs
  signés conservés et renvoyés avec les appels d'outils ; pas via OpenRouter). Le raisonnement
  s'affiche à part (une ligne `∴ Réflexion (5 s)` dans le terminal, le texte complet avec
  `CLAW_SHOW_THINKING=1` ; bloc repliable dans l'interface web) ; `off` le masque.

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
- **Reprise** : `claw --continue` (ou `-c`) reprend la dernière session du projet, `claw --resume`
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
- **GitHub** : l'action `.github/actions/claw` et un exemple de workflow « `@claw` dans un
  commentaire » sont décrits dans [`docs/github-action.md`](docs/github-action.md).
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
  arrière-plan puis donne son adresse `http://localhost`. Limites : l'outil montre la page telle
  qu'elle se charge, sans cliquer ni saisir, et ne rend pas d'image. Il n'est proposé au modèle
  que si un navigateur est installé ; `CLAW_BROWSER` désigne l'exécutable à utiliser.

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
- Découverte d'agents et de skills locaux avec `claw agents` et `claw skills`
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
