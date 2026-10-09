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
| Claw : Accepter la modification proposée | `Ctrl+Entrée` dans le diff | Autorise l'écriture montrée dans le diff |
| Claw : Refuser la modification proposée | `Ctrl+Maj+Retour arrière` dans le diff | Refuse, avec une consigne facultative pour l'agent |
| Claw : Toujours autoriser ce type de modification | menu « … » du diff | Accepte et enregistre la règle proposée par Claw |
| Claw : Voir les modifications du dernier tour | bouton de la barre d'état | Diff « avant Claw / maintenant » des fichiers modifiés |
| Claw : Voir les modifications de la session | | Le même diff, depuis le début de la session |
| Claw : Interrompre le tour en cours | | Arrête l'agent |
| Claw : Ouvrir dans le navigateur | | Même session dans le navigateur |
| Claw : Recharger le panneau | | Relance Claw et recharge le panneau, sur la même session |
| Claw : Arrêter l'agent | | Arrête le serveur local |

Les autorisations, l'historique, les images collées et les commandes (`/review`, `/undo`…) sont
ceux de l'interface web de Claw. Le serveur n'écoute que sur `127.0.0.1` avec un jeton, et la page
n'accepte d'être affichée que dans un panneau de VS Code.

## Modifications proposées : diff de l'éditeur

En mode `prompt`, l'agent demande l'autorisation avant d'écrire ou de modifier un fichier. Le
changement s'ouvre alors dans un diff de VS Code : le fichier actuel à gauche, ce que l'agent
propose à droite (un fichier à créer est comparé à un document vide). Rien n'est écrit tant que
vous n'avez pas répondu.

- **Accepter** ou **Refuser** : boutons de la barre de titre du diff, boutons de la barre d'état
  (visibles tant qu'une modification attend), raccourcis, ou le panneau de Claw comme avant.
- Refuser ouvre une saisie : une consigne pour l'agent (facultative), `Entrée` pour refuser,
  `Échap` pour revenir au diff sans répondre.
- Le diff se ferme dès que la réponse est donnée, d'un côté ou de l'autre.

Dans les modes qui n'interrogent pas (`workspace-write`, `danger-full-access`), les fichiers sont
écrits directement : un bouton `diff` apparaît dans la barre d'état après le tour et ouvre le
diff « avant Claw / maintenant » de chaque fichier modifié. `/undo` les rétablit.

Réglage `claw.nativeDiff` (activé par défaut) : à `false`, seule la demande du panneau s'affiche.

## Problèmes signalés par l'éditeur

L'agent lit ce que VS Code souligne (erreurs de compilation, de types, de lint), sans lancer de
compilation :

- l'outil `EditorDiagnostics` donne au modèle les problèmes d'un fichier ou de tout l'espace de
  travail, quand il le demande ;
- à la fin d'un tour qui a modifié des fichiers, Claw demande à VS Code les erreurs de ces
  fichiers et les renvoie au modèle pour correction (deux fois au plus par tour).

VS Code ouvre les fichiers concernés en arrière-plan et laisse ses analyses se stabiliser (moins
d'une seconde en général, six au plus) avant de répondre. Les problèmes viennent de vos
extensions de langage : sans extension pour un langage, il n'y a rien à lire.

Réglage `claw.editorDiagnostics` (activé par défaut) ; côté Claw, `CLAW_EDITOR_DIAGNOSTICS=0`.

## Mise à jour de claw

Quand l'exécutable `claw` est remplacé (par exemple `cargo install --path crates/claw-cli
--locked`), le panneau s'en aperçoit en quelques secondes : il relance Claw, reprend la session
en cours et se recharge. Si l'agent travaille, il attend la fin du tour (la barre d'état
l'indique).

Sous Windows, un programme en cours d'exécution verrouille son fichier. Le panneau lance donc
une copie de `claw.exe`, gardée dans le dossier de l'extension : l'exécutable installé reste
libre d'être remplacé pendant que le panneau est ouvert.

## Réglages

- `claw.path` : exécutable (défaut `claw`).
- `claw.model` : modèle, par exemple `qwen/qwen3-coder:free` ou `ollama:qwen2.5-coder:7b`.
- `claw.permissionMode` : `prompt`, `read-only`, `workspace-write` ou `danger-full-access`.
- `claw.nativeDiff` : ouvrir les modifications proposées dans un diff de l'éditeur (défaut : oui).
- `claw.editorDiagnostics` : laisser l'agent lire les problèmes de l'éditeur (défaut : oui).

## Tests

Depuis `editors/vscode`, après `npm ci` et `cargo build -p claw-cli` :

- `node test/client.test.js ../../target/debug/claw` : pilotage de `claw serve` sans VS Code.
- `node test/run-vscode.js ../../target/debug/claw` : l'extension dans un vrai VS Code, téléchargé
  à part avec un profil isolé (votre éditeur n'est pas touché), face à un faux modèle
  (`rust/scripts/mock-model.js`). Sous Linux sans écran : `xvfb-run -a node test/run-vscode.js …`.

La CI les exécute sous Linux et Windows.
