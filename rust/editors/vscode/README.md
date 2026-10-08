# Claw Code pour VS Code

L'agent Claw (n'importe quel modèle : OpenRouter, Ollama, Groq…) dans un panneau de VS Code.

## Prérequis

- `claw` installé (`cargo install --path crates/claw-cli --locked`) ou son chemin dans le réglage `claw.path`.
- Une clé (par exemple `OPENROUTER_API_KEY`) dans l'environnement, ou un serveur local (Ollama).

## Commandes

| Commande | Raccourci | Effet |
|---|---|---|
| Claw : Ouvrir l'agent | `Ctrl+Alt+K` | Démarre Claw dans le dossier ouvert et l'affiche à côté de l'éditeur |
| Claw : Envoyer la sélection | `Ctrl+Alt+L` | Envoie le code sélectionné (fichier, lignes) avec votre consigne |
| Claw : Demander à propos de ce fichier | menu contextuel | Joint le fichier (`@chemin`) à votre demande |
| Claw : Poser une question | | Envoie une demande sans quitter l'éditeur |
| Claw : Interrompre le tour en cours | | Arrête l'agent |
| Claw : Ouvrir dans le navigateur | | Même session dans le navigateur |
| Claw : Arrêter l'agent | | Arrête le serveur local |

Les autorisations, l'historique, les images collées et les commandes (`/review`, `/undo`…) sont
ceux de l'interface web de Claw. Le serveur n'écoute que sur `127.0.0.1` avec un jeton, et la page
n'accepte d'être affichée que dans un panneau de VS Code.

## Réglages

- `claw.path` : exécutable (défaut `claw`).
- `claw.model` : modèle, par exemple `qwen/qwen3-coder:free` ou `ollama:qwen2.5-coder:7b`.
- `claw.permissionMode` : `prompt`, `read-only`, `workspace-write` ou `danger-full-access`.

## Tests

Depuis `editors/vscode`, après `npm ci` et `cargo build -p claw-cli` :

- `node test/client.test.js ../../target/debug/claw` : pilotage de `claw serve` sans VS Code.
- `node test/run-vscode.js ../../target/debug/claw` : l'extension dans un vrai VS Code, téléchargé
  à part avec un profil isolé (votre éditeur n'est pas touché), face à un faux modèle
  (`rust/scripts/mock-model.js`). Sous Linux sans écran : `xvfb-run -a node test/run-vscode.js …`.

La CI les exécute sous Linux et Windows.
