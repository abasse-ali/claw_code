Écrire un workflow

Un workflow est un script JavaScript que Claw exécute en arrière-plan : il lance des agents, par
phases, et rend un seul résultat, qui revient dans la conversation entre deux tours.

  export const meta = {
    name: 'audit-routes',
    description: 'Vérifie les contrôles d'accès de chaque route',
    phases: [{ title: 'Inventaire' }, { title: 'Audit' }],
  }

  phase('Inventaire')
  const found = await agent('Liste les fichiers .ts de src/routes.', {
    schema: { type: 'object', required: ['files'],
              properties: { files: { type: 'array', items: { type: 'string' } } } },
  })
  phase('Audit')
  const audits = await pipeline(found.files, (file) =>
    agent(`Cherche les contrôles d'accès manquants dans ${file}.`, { label: file }))
  return audits.filter(Boolean)

Règles
  meta              en tête, littéral pur : name et description, phases facultatif
  corps             JavaScript ordinaire ; await et return au premier niveau
  agent(p, o)       un agent avec ses outils ; rend son rapport (texte), la valeur JSON
                    conforme à o.schema (JSON Schema), ou null en cas d'échec
                    options : label, phase, schema, model, agentType, isolation ('worktree')
  parallel([...])   fonctions lancées ensemble ; une erreur devient null
  pipeline(l, ...)  chaque élément passe par les étapes ; null arrête cet élément
  phase(titre)      range les agents suivants sous ce titre
  log(...)          ligne visible dans /workflows <id>
  args              valeur passée au lancement (/nom texte, ou args de l'outil Workflow)

Le script ne voit ni fichiers, ni réseau, ni processus (contexte vm de Node.js, lancé avec le
modèle de permissions de Node) : tout passe par les agents, qui gardent les permissions de la
session et ne peuvent rien demander à l'utilisateur (une action à approuver est refusée).

Lancer et suivre
  demander « ultracode » ou « utilise un workflow » : le modèle écrit et lance le script
  /deep-research <question>      workflow intégré (recherche web recoupée, rapport cité)
  /<nom> [arguments]             workflow enregistré
  /workflows                     exécutions et workflows enregistrés
  /workflows <id>                phases, agents, journal, résultat
  /workflows stop <id>           arrêt
  /workflows save <id> [nom]     enregistre le script dans .claw/workflows/<nom>.js
  reprise                        outil Workflow avec resumeFromRunId : les agents déjà finis
                                 avec les mêmes entrées rendent leur résultat gardé

Emplacements : .claw/workflows et .claude/workflows (projet), ~/.claw/workflows et
~/.claude/workflows. Il faut Node.js 20 ou plus.

Modèles gratuits : chaque agent coûte une dizaine de requêtes. CLAW_WORKFLOW_MAX_AGENTS (30)
limite les agents d'une exécution, CLAW_WORKFLOW_MAX_CONCURRENT_AGENTS (3) ceux qui tournent
ensemble, CLAW_WORKFLOW_SUBAGENT_MODEL impose leur modèle. CLAW_DISABLE_WORKFLOWS=1 ou
"disableWorkflows": true coupe les workflows.
