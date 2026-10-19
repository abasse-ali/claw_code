# Sessions de Claw Web sur votre PC

Les sessions de Claw Web tournent d'ordinaire sur une machine de GitHub, dans le dépôt privé des
sessions (`claw-sessions`) : elles consomment les minutes GitHub Actions du compte (2 000 par mois
avec GitHub Free). Ici, elles tournent sur votre PC, dans Docker Desktop, avec un exécuteur
auto-hébergé de GitHub : aucune minute n'est comptée.

## Ce qu'il faut

- Docker Desktop (moteur Linux, WSL 2), lancé.
- GitHub CLI connecté (`gh auth login`) avec un compte administrateur du dépôt des sessions.
- Environ 4 Go d'espace disque pour l'image (Linux, Python, git, Rust, Node, Chrome et
  l'exécuteur de GitHub).

## Installer

Dans PowerShell, depuis ce dossier :

```powershell
powershell -ExecutionPolicy Bypass -File .\claw-pc.ps1 install
```

Le script construit l'image (plusieurs minutes la première fois), inscrit l'exécuteur `claw-pc`
auprès de `abasse-ali/claw-sessions` (autre dépôt : `-Engine propriétaire/nom`) et le lance.
Le jeton d'inscription n'est jamais affiché. Ensuite, dans l'appli : Réglages, Sessions, « Sur mon
PC ».

Pour que les sessions partent dès que le PC est allumé : Docker Desktop, Settings, General,
« Start Docker Desktop when you sign in ». Le conteneur redémarre avec Docker.

## Commandes

| Commande | Effet |
|---|---|
| `.\claw-pc.ps1 status` | état du conteneur, et de l'exécuteur vu par GitHub (en ligne, en session) |
| `.\claw-pc.ps1 stop` | arrête l'exécuteur ; les sessions attendent dans la file de GitHub |
| `.\claw-pc.ps1 start` | le relance ; les sessions en attente partent |
| `.\claw-pc.ps1 remove` | le désinscrit de GitHub, puis supprime conteneur, volume et image |

## Ce qui change par rapport à une machine de GitHub

- Une machine de GitHub repart de zéro à chaque session ; l'exécuteur du PC garde ses fichiers.
  Le moteur vide donc le dossier de travail au début de chaque session, et les réglages, la
  mémoire et les caches de Claw vivent dans le dossier temporaire du job (`CLAW_CONFIG_HOME`),
  vidé à chaque session : rien d'une session d'un dépôt ne passe à la suivante.
- Claw tourne avec toutes les autorisations, comme sur GitHub, mais dans le conteneur, jamais en
  root : il ne voit pas vos fichiers Windows.
- Une session attend tant que le PC est éteint ou Docker Desktop arrêté, puis part seule.
- L'exécuteur ne sert que le dépôt privé des sessions. GitHub déconseille un exécuteur
  auto-hébergé pour un dépôt public : n'importe qui pourrait y faire tourner du code.

## Workflow du dépôt des sessions

L'appli passe le choix de la machine au workflow `claw-web.yml` du dépôt des sessions, qui le
transmet au moteur. Ce workflow doit connaître l'entrée `runner` :

```yaml
on:
  workflow_dispatch:
    inputs:
      # … entrées existantes (repo, base, session, model, team, ultracode)
      runner:
        description: "Machine (vide : GitHub ; claw-pc : votre PC)"
        required: false
        type: string
        default: ""

jobs:
  task:
    uses: abasse-ali/claw_code/.github/workflows/claw-web.yml@main
    with:
      # … entrées existantes
      runner: ${{ inputs.runner }}
    secrets: inherit
```

Sans elle, l'appli dit que le workflow ne connaît pas encore « Sur mon PC ».
