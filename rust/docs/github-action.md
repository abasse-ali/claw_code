# Claw dans GitHub (`@claw` dans un commentaire)

L'action `.github/actions/claw` lance l'agent sur une demande et renvoie sa réponse
(`result`) et un indicateur de modifications (`changed`). L'exemple ci-dessous
répond quand quelqu'un écrit `@claw …` dans une issue ou une pull request : il
publie la réponse en commentaire et, si l'agent a modifié des fichiers, ouvre une
pull request.

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
        uses: ./.github/actions/claw
        with:
          openrouter-api-key: ${{ secrets.OPENROUTER_API_KEY }}
          permission-mode: workspace-write
          prompt: |
            Tâche demandée dans ${{ github.event.issue.html_url }} :
            Titre : ${{ github.event.issue.title }}
            ${{ github.event.issue.body }}

            Demande : ${{ github.event.comment.body }}

      - name: Ouvrir une pull request si des fichiers ont changé
        if: steps.claw.outputs.changed == 'true'
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          branch="claw/issue-${{ github.event.issue.number }}-${{ github.run_id }}"
          git config user.name "claw-bot"
          git config user.email "claw-bot@users.noreply.github.com"
          git checkout -b "$branch"
          git add -A
          git commit -m "Claw : ${{ github.event.issue.title }}"
          git push origin "$branch"
          gh pr create --title "Claw : ${{ github.event.issue.title }}" --body "Proposé par Claw pour #${{ github.event.issue.number }}." --head "$branch"

      - name: Répondre dans le fil
        env:
          GH_TOKEN: ${{ github.token }}
          RESULT: ${{ steps.claw.outputs.result }}
        run: gh issue comment ${{ github.event.issue.number }} --body "$RESULT"
```

Réglages utiles : `model` (sinon le meilleur modèle gratuit pour le code),
`permission-mode: read-only` pour une revue sans modification, `install: release`
pour utiliser le binaire de la dernière release au lieu de compiler (plus rapide).
