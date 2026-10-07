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

Sous Windows, l'outil `bash` et les hooks utilisent le bash de Git for Windows,
détecté automatiquement ; `CLAW_SHELL` permet d'indiquer un autre `bash.exe`.

### Interface web : `claw serve`

```bash
claw serve                 # ouvre http://127.0.0.1:8787/?token=… dans le navigateur
claw serve --port 9000 --no-open
```

Même agent que dans le terminal (modèle, permissions, MCP, sous-agents, sessions), dans une page :
réponses en direct avec Markdown, tableaux et code ; cartes d'outils repliables (diffs en
couleur pour les éditions, sortie des commandes) ; **demandes de permission à valider dans la
page** (une fois / toujours / refuser avec consigne) ; diagrammes Mermaid affichés à la demande ;
choix du modèle parmi les gratuits recommandés pour le code, du mode de permission, nouvelle
conversation et reprise des sessions ; commandes `/review`, `/audit`, `/archi`, `/adr`, `/fix`,
`/models`, `/mcp`, `/cost`, `/compact`…

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
  `/models <recherche>` filtre, `/model <id>` change de modèle.
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
À chaque demande : `o` (oui, une fois), `t` (toujours : enregistre la règle proposée dans
`.claw/settings.local.json`), `n` (non), ou une consigne libre transmise à l'agent.

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
  automatiquement) et `http` (Streamable HTTP, réponses JSON ou SSE, session `Mcp-Session-Id`).
  `sse` (ancien protocole), `ws` et l'OAuth MCP ne sont pas encore pris en charge.
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
| `SessionStart` / `SessionEnd` | début (`startup`, `resume`, `clear`) / fin de session | — |
| `PreCompact` | avant une compaction (`auto`, `manual`) | — |
| `Notification` | quand une autorisation est demandée (en arrière-plan) | — |

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

### Langue des réponses

`"language": "français"` dans la config (par exemple `~/.claw/settings.json`) fixe la langue des
réponses, y compris pour les commandes `/review`, `/audit`…

La connexion OAuth est aussi disponible :

```bash
cargo run --bin claw -- login
```

### Installation locale

```bash
cargo install --path crates/claw-cli --locked
```

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

- `claw-cli` — binaire orienté utilisateur
- `api` — clients de fournisseurs et streaming
- `runtime` — sessions, config, permissions, prompts et boucle d'exécution
- `tools` — implémentations des outils intégrés
- `commands` — registre et handlers des commandes slash
- `plugins` — découverte, registre et support de cycle de vie des plugins
- `lsp` — types de support du protocole de langage et helpers de processus
- `server` et `compat-harness` — services de support et outillage de compatibilité

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
