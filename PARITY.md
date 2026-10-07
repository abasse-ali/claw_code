# ANALYSE DES ÉCARTS DE PARITÉ

Périmètre : comparaison en lecture seule entre la source TypeScript originale à `/home/bellman/Workspace/claw-code/src/` et le portage Rust sous `rust/crates/`.

Méthode : comparaison des surfaces de fonctionnalités, des registres, des points d'entrée et de la plomberie d'exécution uniquement. Aucune source TypeScript n'a été copiée.

## Résumé exécutif

Le portage Rust dispose d'une bonne base pour :
- les bases API/OAuth d'Anthropic
- l'état local de conversation/session
- une boucle d'outils centrale
- le support stdio/bootstrap MCP
- la découverte de CLAW.md
- un petit ensemble d'outils intégrés mais utilisable

Il n'est **pas en parité de fonctionnalités** avec la CLI TypeScript.

Plus grands écarts :
- les **plugins** sont effectivement absents en Rust
- les **hooks** sont parsés mais pas exécutés en Rust
- l'**étendue de la CLI** est bien plus étroite en Rust
- les **skills** ne fonctionnent qu'à partir de fichiers locaux en Rust, sans le pipeline registre/intégré du TS
- l'**orchestration de l'assistant** n'a pas l'orchestration consciente des hooks du TS ni les transports distants/structurés
- les **services** au-delà du cœur API/OAuth/MCP sont en grande partie manquants en Rust

---

## tools/

### Existe en TS
Preuves :
- `src/tools/` contient de larges familles d'outils incluant `AgentTool`, `AskUserQuestionTool`, `BashTool`, `ConfigTool`, `FileReadTool`, `FileWriteTool`, `GlobTool`, `GrepTool`, `LSPTool`, `ListMcpResourcesTool`, `MCPTool`, `McpAuthTool`, `ReadMcpResourceTool`, `RemoteTriggerTool`, `ScheduleCronTool`, `SkillTool`, `Task*`, `Team*`, `TodoWriteTool`, `ToolSearchTool`, `WebFetchTool`, `WebSearchTool`.
- L'exécution/orchestration des outils est répartie entre `src/services/tools/StreamingToolExecutor.ts`, `src/services/tools/toolExecution.ts`, `src/services/tools/toolHooks.ts` et `src/services/tools/toolOrchestration.ts`.

### Existe en Rust
Preuves :
- Le registre d'outils est centralisé dans `rust/crates/tools/src/lib.rs` via `mvp_tool_specs()`.
- Les outils intégrés actuels comprennent les primitives shell/file/search/web/todo/skill/agent/config/notebook/repl/powershell.
- L'exécution au runtime est câblée via `rust/crates/tools/src/lib.rs` et `rust/crates/runtime/src/conversation.rs`.

### Manquant ou cassé en Rust
- Pas d'équivalents Rust pour les principaux outils TS tels que `AskUserQuestionTool`, `LSPTool`, `ListMcpResourcesTool`, `MCPTool`, `McpAuthTool`, `ReadMcpResourceTool`, `RemoteTriggerTool`, `ScheduleCronTool`, `Task*`, `Team*` et plusieurs outils de workflow/système.
- La surface d'outils Rust est encore explicitement un registre MVP, pas un registre de parité.
- Rust n'a pas la séparation en couches d'orchestration d'outils du TS.

**Statut :** noyau partiel seulement.

---

## hooks/

### Existe en TS
Preuves :
- Surface de commande des hooks sous `src/commands/hooks/`.
- Mécanique de hooks au runtime dans `src/services/tools/toolHooks.ts` et `src/services/tools/toolExecution.ts`.
- Le TS prend en charge `PreToolUse`, `PostToolUse` et des comportements pilotés par hooks plus larges configurés via les paramètres et documentés dans `src/skills/bundled/updateConfig.ts`.

### Existe en Rust
Preuves :
- La config des hooks est parsée et fusionnée dans `rust/crates/runtime/src/config.rs`.
- La config des hooks peut être inspectée via le rapport de config Rust dans `rust/crates/commands/src/lib.rs` et `rust/crates/claw-cli/src/main.rs`.
- Les directives de prompt mentionnent les hooks dans `rust/crates/runtime/src/prompt.rs`.

### Manquant ou cassé en Rust
- Pas de pipeline d'exécution de hooks réel dans `rust/crates/runtime/src/conversation.rs`.
- Pas de comportement de mutation/déni/réécriture/hook-de-résultat PreToolUse/PostToolUse.
- Pas de commande Rust `/hooks` en parité.

**Statut :** configuration seulement ; comportement runtime manquant.

---

## plugins/

### Existe en TS
Preuves :
- Échafaudage de plugins intégrés dans `src/plugins/builtinPlugins.ts` et `src/plugins/bundled/index.ts`.
- Cycle de vie/services de plugins dans `src/services/plugins/PluginInstallationManager.ts` et `src/services/plugins/pluginOperations.ts`.
- Surface de commande CLI/plugin sous `src/commands/plugin/` et `src/commands/reload-plugins/`.

### Existe en Rust
Preuves :
- Aucun sous-système de plugin dédié n'apparaît sous `rust/crates/`.
- Les références Rust aux plugins à l'échelle du dépôt sont effectivement absentes au-delà des mentions de texte/aide.

### Manquant ou cassé en Rust
- Pas de chargeur de plugins.
- Pas de flux d'installation/mise à jour/activation/désactivation depuis une place de marché.
- Pas de parité `/plugin` ou `/reload-plugins`.
- Pas de voie d'extension de hook/outil/commande/MCP fournie par les plugins.

**Statut :** manquant.

---

## skills/ et découverte de CLAW.md

### Existe en TS
Preuves :
- Pipeline de chargement/registre des skills dans `src/skills/loadSkillsDir.ts`, `src/skills/bundledSkills.ts` et `src/skills/mcpSkillBuilders.ts`.
- Skills intégrés sous `src/skills/bundled/`.
- Surface de commande des skills sous `src/commands/skills/`.

### Existe en Rust
Preuves :
- L'outil `Skill` dans `rust/crates/tools/src/lib.rs` résout et lit les fichiers `SKILL.md` locaux.
- La découverte de CLAW.md est implémentée dans `rust/crates/runtime/src/prompt.rs`.
- Rust prend en charge `/memory` et `/init` via `rust/crates/commands/src/lib.rs` et `rust/crates/claw-cli/src/main.rs`.

### Manquant ou cassé en Rust
- Pas d'équivalent de registre de skills intégrés.
- Pas de commande `/skills`.
- Pas de pipeline de constructeur de skills MCP.
- Pas de gestion de découverte/rechargement/changement de skills en direct façon TS.
- Pas d'intégration mémoire-de-session / mémoire-d'équipe comparable autour des skills.

**Statut :** chargement basique de skills locaux uniquement.

---

## cli/

### Existe en TS
Preuves :
- Vaste surface de commandes sous `src/commands/` incluant `agents`, `hooks`, `mcp`, `memory`, `model`, `permissions`, `plan`, `plugin`, `resume`, `review`, `skills`, `tasks`, et bien d'autres.
- Pile de transport structuré/distant dans `src/cli/structuredIO.ts`, `src/cli/remoteIO.ts` et `src/cli/transports/*`.
- Séparation des handlers CLI dans `src/cli/handlers/*`.

### Existe en Rust
Preuves :
- Registre partagé des commandes slash dans `rust/crates/commands/src/lib.rs`.
- Les commandes slash Rust couvrent actuellement `help`, `status`, `compact`, `model`, `permissions`, `clear`, `cost`, `resume`, `config`, `memory`, `init`, `diff`, `version`, `export`, `session`.
- La gestion principale CLI/REPL/prompt vit dans `rust/crates/claw-cli/src/main.rs`.

### Manquant ou cassé en Rust
- Familles de commandes TS majeures manquantes : `/agents`, `/hooks`, `/mcp`, `/plugin`, `/skills`, `/plan`, `/review`, `/tasks`, et bien d'autres.
- Pas d'équivalent Rust aux couches IO structurées / transport distant du TS.
- Pas de décomposition de handlers façon TS pour auth/plugins/MCP/agents.
- Le mode prompt JSON est amélioré sur cette branche, mais reste sans parité de transport propre : la vérification empirique montre que la sortie JSON compatible outils peut émettre des lignes de résultat d'outils lisibles par un humain avant l'objet JSON final.

**Statut :** noyau CLI local fonctionnel, bien plus étroit que le TS.

---

## assistant/ (boucle agentique, streaming, appel d'outils)

### Existe en TS
Preuves :
- Surface assistant/session dans `src/assistant/sessionHistory.ts`.
- Orchestration d'outils dans `src/services/tools/StreamingToolExecutor.ts`, `src/services/tools/toolExecution.ts`, `src/services/tools/toolOrchestration.ts`.
- Couches de streaming distant/structuré dans `src/cli/structuredIO.ts` et `src/cli/remoteIO.ts`.

### Existe en Rust
Preuves :
- Boucle centrale dans `rust/crates/runtime/src/conversation.rs`.
- Traduction d'événements stream/outils dans `rust/crates/claw-cli/src/main.rs`.
- Persistance de session dans `rust/crates/runtime/src/session.rs`.

### Manquant ou cassé en Rust
- Pas de couche d'orchestration consciente des hooks façon TS.
- Pas de pile de transport assistant structuré/distant façon TS.
- Pas d'intégration plus riche assistant/historique-de-session/tâche-d'arrière-plan façon TS.
- Le chemin de sortie JSON n'est plus limité à un seul tour sur cette branche, mais la propreté de sortie reste en deçà des attentes de transport TS.

**Statut :** boucle centrale solide, couches d'orchestration manquantes.

---

## services/ (client API, auth, modèles, MCP)

### Existe en TS
Preuves :
- Services API sous `src/services/api/*`.
- Services OAuth sous `src/services/oauth/*`.
- Services MCP sous `src/services/mcp/*`.
- Couches de services supplémentaires pour les analytics, la suggestion de prompts, la mémoire de session, les opérations de plugins, la synchronisation des paramètres, les limites de politique, la synchronisation de mémoire d'équipe, le notifieur, la voix, et plus sous `src/services/*`.

### Existe en Rust
Preuves :
- Client API Anthropic central dans `rust/crates/api/src/{client,error,sse,types}.rs`.
- Support OAuth dans `rust/crates/runtime/src/oauth.rs`.
- Support MCP config/bootstrap/client dans `rust/crates/runtime/src/{config,mcp,mcp_client,mcp_stdio}.rs`.
- Comptabilisation de l'usage dans `rust/crates/runtime/src/usage.rs`.
- Support proxy-amont distant dans `rust/crates/runtime/src/remote.rs`.

### Manquant ou cassé en Rust
- La plupart de l'écosystème de services TS au-delà du cœur messaging/auth/MCP est absent.
- Pas de couche de service de plugins équivalente au TS.
- Pas de sous-systèmes analytics/synchronisation-paramètres/limite-politique/mémoire-équipe équivalents au TS.
- Pas de couche gestionnaire-de-connexion/UI MCP façon TS.
- L'ergonomie modèle/fournisseur reste plus mince qu'en TS.

**Statut :** fondation centrale présente ; écosystème de services plus large manquant.

---

## Statut des bugs critiques dans cet arbre de travail

### Corrigés
- **Outils activés en mode prompt**
  - `rust/crates/claw-cli/src/main.rs` construit maintenant le mode prompt avec `LiveCli::new(model, true, ...)`.
- **Mode de permission par défaut = DangerFullAccess**
  - La valeur par défaut du runtime se résout maintenant à `DangerFullAccess` dans `rust/crates/claw-cli/src/main.rs`.
  - La valeur par défaut Clap utilise aussi `DangerFullAccess` dans `rust/crates/claw-cli/src/args.rs`.
  - Le template d'initialisation écrit `dontAsk` dans `rust/crates/claw-cli/src/init.rs`.
- **Bug du préfixe d'entrée outil `{}` en streaming**
  - `rust/crates/claw-cli/src/main.rs` retire désormais l'objet vide initial uniquement pour l'entrée d'outil en streaming, tout en préservant les `{}` légitimes dans les réponses non streamées.
- **max_iterations illimité**
  - Vérifié dans `rust/crates/runtime/src/conversation.rs` avec `usize::MAX`.

### Problème de parité notable restant
- **Propreté de la sortie prompt JSON**
  - Le mode JSON compatible outils boucle maintenant, mais la vérification empirique montre toujours une sortie de résultat d'outils lisible par un humain avant le JSON lorsque des outils sont déclenchés.
