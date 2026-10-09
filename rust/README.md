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

**Windows : « clé manquante » alors que la clé est définie.** Un programme garde l'environnement
reçu à son démarrage : VS Code ouvert par le clic droit « Ouvrir avec Code », un terminal resté
ouvert ou une tâche planifiée peuvent ignorer une variable créée depuis. Claw relit donc au
démarrage les variables de votre compte dans le registre (`HKCU\Environment`, puis celles de la
machine si aucune clé n'est trouvée) et adopte celles qui lui servent et qui manquent : clés
(`*_API_KEY`, `*_AUTH_TOKEN`), adresses (`*_BASE_URL`), `CLAW_*`, `OLLAMA_*`. Une variable déjà
présente n'est jamais remplacée, et aucune valeur n'est affichée. `claw doctor` le signale
(« clé lue dans les variables du compte Windows ») ; `CLAW_NO_USER_ENV=1` coupe cette reprise.

Sous Windows, l'outil `bash` et les hooks utilisent le bash de Git for Windows,
détecté automatiquement ; `CLAW_SHELL` permet d'indiquer un autre `bash.exe`.

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
  modification), dépliable sur le diff ligne à ligne, la sortie, le contenu lu ou la liste de
  tâches ; la barre d'activité au-dessus de la saisie dit ce que fait l'agent ;
- **demandes de permission dans la page**, au clavier : `Entrée` autoriser, `A` toujours,
  `Échap` refuser (avec consigne facultative). La demande en attente est aussi **épinglée
  au-dessus de la zone de saisie**, avec ses boutons : elle reste visible où que soit le fil,
  même sur un petit écran ou dans une fenêtre réduite ;
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
  modèle de ce classement.
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

- **Compaction automatique** : à l'approche de la fenêtre de contexte du modèle, les anciens
  messages sont résumés par le modèle (les plus récents sont gardés tels quels). Seuil réglable
  avec `CLAW_AUTO_COMPACT_TOKENS`, désactivation avec `CLAW_AUTO_COMPACT=0`.
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
  identifiant ; il devient le modèle de la session) ou d'arrêter le tour. La question n'attend
  votre réponse que si les essais automatiques sont épuisés, ou si réessayer ne servirait à rien
  (quota du jour épuisé, clé refusée, modèle introuvable). Quand l'agent n'avance plus (voir
  « Escalade »), la même barre propose un autre modèle ; sans réponse en 60 s, le même modèle
  continue. `"modelSwitch": "auto"` (ou `CLAW_MODEL_SWITCH=auto`) rétablit la bascule sans
  question décrite ci-dessous ; elle s'applique aussi d'office quand personne n'est là pour
  répondre (`claw prompt` dans un script, action GitHub, sous-agents).
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
- `"smallModel"` (ou `CLAW_SMALL_MODEL`) : petit modèle utilisé pour les résumés de compaction.

### Fiabilité avec les petits modèles

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

### Aides au raisonnement

Un grand modèle vérifie son travail de lui-même ; un petit modèle gratuit l'oublie, s'égare ou
tourne en rond. Ces aides ne sont pas des consignes ajoutées au prompt : c'est **Claw qui les
applique**, quel que soit le modèle.

| Aide | Ce que fait Claw | Couper |
|---|---|---|
| Vérification imposée | Quand le modèle dit avoir fini après avoir modifié des fichiers, Claw lance la vérification du projet (`cargo check`, `go build`, `tsc`, tests Python… ou `"verify"`). En cas d'échec, la sortie repart au modèle, qui corrige (3 corrections au plus) ; si cela échoue encore, Claw l'annonce : « le travail n'est pas validé » | `CLAW_VERIFY=0` |
| Syntaxe immédiate | Après chaque modification, contrôle de syntaxe du fichier (JSON, Python, JavaScript, Rust, Go, shell) ; l'erreur est jointe au résultat de l'outil | `CLAW_DIAGNOSTICS=0` |
| Détection de blocage | Trois appels identiques sans effet, deux modifications ratées du même fichier, quatre échecs d'affilée, une vérification relancée alors que rien n'a changé, 40 appels dans un tour : Claw recadre le modèle | `CLAW_STALL_GUARD=0` |
| Test réécrit | Un test (ou le script de vérification) modifié après un échec est signalé à l'utilisateur et au modèle : faire passer un contrôle en le réécrivant ne prouve rien | `CLAW_TEST_GUARD=0` |
| Carte du projet | Fichiers et principaux symboles du projet, donnés au début de la session (6 000 caractères, `CLAW_REPO_MAP_CHARS`) | `CLAW_REPO_MAP=0` |
| Chemins | Un chemin inventé (`/workspace/src/a.py`, `/c/…`, `/mnt/c/…`) est retrouvé s'il existe dans le projet ; sinon l'erreur donne le dossier de travail et les fichiers au nom proche | `CLAW_PATH_HELP=0` |
| Outils utiles seulement | Les outils rarement utiles ne sont proposés que lorsqu'ils servent (`BashOutput` s'il y a une tâche de fond, `NotebookEdit` s'il y a un notebook…) | `CLAW_TOOL_DIET=0` |
| Raisonnement conservé | Le raisonnement du modèle est renvoyé à l'étape suivante du même tour (OpenRouter), au lieu d'être perdu entre deux appels d'outils | `CLAW_REASONING_PASSBACK=0` |
| Relecture | Avant de rendre la main, les modifications d'au moins 5 lignes sont relues par **un autre modèle** ; ses remarques repartent au modèle, une fois par tour | `CLAW_REVIEW=0` ou `"review": false` |
| Conseil | Au deuxième échec de la vérification, ou au deuxième recadrage, un autre modèle lit la fin de la session et donne son diagnostic | `CLAW_COUNCIL=0` |
| Second avis | L'outil `Consult` (et `/avis <question>`) pose la même question à plusieurs autres modèles ; l'agent compare et décide | `CLAW_COUNCIL=0` |
| Leçons | L'outil `Remember` note une leçon utile au projet dans `.claw/lessons.md` (80 au plus), relue au début de chaque session | `CLAW_LESSONS=0` |
| Modèle de planification | En mode plan, `"plannerModel"` réfléchit à la place du modèle de la session, qui exécute ensuite le plan approuvé | `CLAW_PLANNER=0` |
| Escalade | Au deuxième échec de la vérification, ou au deuxième recadrage, Claw vous propose de confier la suite à un autre modèle (le gratuit le plus puissant en premier) ; sans réponse en 60 s, le même modèle continue. Avec `"modelSwitch": "auto"` ou sans personne pour répondre, **le modèle gratuit le plus puissant du moment** reprend la fin du tour ; le tour suivant repart avec le modèle habituel. Sans réglage à faire, et jamais vers un modèle payant | `CLAW_ESCALATION=0` |
| Erreurs de l'éditeur | Avec l'extension VS Code : quand le tour a modifié des fichiers, Claw demande à l'éditeur les erreurs qu'il y voit (compilation, types, lint) et les renvoie au modèle, deux fois au plus par tour. L'outil `EditorDiagnostics` permet aussi au modèle de les lire quand il veut | `CLAW_EDITOR_DIAGNOSTICS=0` |
| Contrôle des exigences | Quand le tour a modifié au moins 5 lignes et que la vérification passe, le modèle liste chaque exigence de la demande et de la documentation du code (cas limites compris), retrouve le code qui la couvre et la façon dont elle est vérifiée (un test existant, une commande lancée, ou le code lui-même), et corrige ce qui manque, avant la relecture. Il n'invente ni tests ni résultats. Une requête de plus par tour concerné | `CLAW_CHECKLIST=0` |

**Une seule réponse finale.** Quand un contrôle renvoie le modèle au travail (vérification en
échec, erreurs de l'éditeur, exigences, relecture), la réponse qu'il venait de donner n'était que
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
  "escalationModel": "free"
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
- **Raisonnement** : `/think off|low|medium|high` (ou `"reasoning"` dans la config,
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
  sans argument propose la liste des sessions récentes.
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
réponses, y compris pour les commandes `/review`, `/audit`…

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
