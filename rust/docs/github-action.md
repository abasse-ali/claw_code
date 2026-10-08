# Claw dans GitHub (`@claw` dans un commentaire)

L'action `.github/actions/claw` lance l'agent sur une demande et renvoie sa réponse
(`result`) et un indicateur de modifications (`changed`, sans compter le dossier
`.claw/` des sessions). L'exemple ci-dessous répond quand un membre du dépôt écrit
`@claw …` dans une issue ou une pull request : il publie la réponse en commentaire
et, si l'agent a modifié des fichiers, ouvre une pull request.

1. Ajoutez le secret `OPENROUTER_API_KEY` (Settings → Secrets and variables → Actions).
2. Copiez ce fichier dans `.github/workflows/claw-assistant.yml`.

```yaml
name: Claw assistant

on:
  issue_comment:
    types: [created]

permissions:
  contents: write
  issues: write
  pull-requests: write

jobs:
  claw:
    # Seulement les membres du dépôt, pour qu'un inconnu ne puisse pas lancer l'agent.
    if: contains(github.event.comment.body, '@claw') && contains(fromJSON('["OWNER","MEMBER","COLLABORATOR"]'), github.event.comment.author_association)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - id: claw
        uses: abasse-ali/claw_code/.github/actions/claw@main
        with:
          openrouter-api-key: ${{ secrets.OPENROUTER_API_KEY }}
          permission-mode: workspace-write
          prompt: |
            Tâche demandée dans ${{ github.event.issue.html_url }} :
            Titre : ${{ github.event.issue.title }}
            ${{ github.event.issue.body }}

            Demande : ${{ github.event.comment.body }}

      # Le titre et le numéro passent par l'environnement, jamais directement dans
      # le script : un titre d'issue comme `"; curl … | sh; "` serait sinon exécuté.
      - name: Ouvrir une pull request si des fichiers ont changé
        if: steps.claw.outputs.changed == 'true'
        env:
          GH_TOKEN: ${{ github.token }}
          ISSUE_NUMBER: ${{ github.event.issue.number }}
          ISSUE_TITLE: ${{ github.event.issue.title }}
        run: |
          branch="claw/issue-$ISSUE_NUMBER-$GITHUB_RUN_ID"
          git config user.name "claw-bot"
          git config user.email "claw-bot@users.noreply.github.com"
          git checkout -b "$branch"
          git add -A -- . ':(exclude).claw'
          git commit -m "Claw : $ISSUE_TITLE"
          git push origin "$branch"
          gh pr create --title "Claw : $ISSUE_TITLE" --body "Proposé par Claw pour #$ISSUE_NUMBER." --head "$branch"

      - name: Répondre dans le fil
        env:
          GH_TOKEN: ${{ github.token }}
          ISSUE_NUMBER: ${{ github.event.issue.number }}
          RESULT: ${{ steps.claw.outputs.result }}
        run: gh issue comment "$ISSUE_NUMBER" --body "$RESULT"
```

Dans le dépôt de Claw lui-même (ou un fork), `uses: ./.github/actions/claw` utilise
la version du dépôt.

Réglages utiles :

- `model` : sinon le meilleur modèle gratuit pour le code. Les autres fournisseurs
  (`groq:…`, `mistral:…`, `ollama:…`) lisent leur clé dans l'environnement du job
  (`GROQ_API_KEY: ${{ secrets.GROQ_API_KEY }}`…).
- `permission-mode: read-only` pour une revue sans modification.
- En `workspace-write`, personne ne peut approuver une commande pendant le job : les commandes
  shell sont refusées, l'agent ne peut donc pas lancer les tests. Autorisez celles dont il a
  besoin dans `.claw/settings.json` du dépôt :
  `{"permissions": {"allow": ["Bash(cargo test:*)", "Bash(npm test:*)"]}}`.
- `install: release` pour télécharger le binaire de la dernière release de Claw
  (Linux x64, Windows x64) au lieu de le compiler, ce qui est bien plus
  rapide. Il faut qu'une release existe (tag `v*`).

L'action échoue si l'agent échoue (clé absente, modèle indisponible…) ; ses sorties
restent renseignées.

Le workflow « Intégration » vérifie l'action quand elle change (ou à la demande)
avec un faux modèle (`rust/scripts/mock-model.js`) : compilation, appel d'outil, `result` et
`changed`.
