# Suivi : rendre Claw intelligent pour les modèles gratuits

Ce fichier est le point d'entrée de toute session qui reprend ce chantier. Il dit où on en est,
ce qui reste à faire et comment vérifier. Il est tenu à jour à chaque lot de travail.

## But

On ne peut pas modifier les modèles gratuits que Claw fait tourner. C'est donc Claw qui doit
compenser leurs faiblesses :

- les guider au bon moment, par des remarques courtes et précises, plutôt que par une longue
  consigne générale qu'ils oublient ;
- faire lui-même ce qu'un programme fait mieux qu'un modèle : vérifier, relire, lancer les
  tests, montrer le code en cause, montrer une page ;
- donner à chaque erreur une solution sérieuse : un message qui dit ce qui s'est passé, pourquoi,
  et quoi faire ensuite, ou mieux, une reprise automatique par Claw.

## Reprendre le chantier dans une nouvelle session

1. Lire ce fichier en entier, puis `git log --oneline -15`.
2. Lire les constats en attente dans [suivi/](suivi/) : un fichier par étude, écrit au fil de
   l'eau par les agents. Un constat n'est corrigé qu'après vérification dans le code.
3. Prendre le premier élément « à faire » du tableau ci-dessous, le traiter, le vérifier, le
   committer, puis mettre ce fichier à jour dans le même commit.

Règles permanentes (détail dans la mémoire du projet) :

- Répondre en français. Textes de l'interface en français, remarques destinées au modèle en
  anglais et préfixées `[Claw]`.
- Aucun emoji et aucun tiret long, nulle part.
- Modèles gratuits seulement. Aucune requête à un vrai modèle sans accord explicite : tout se
  teste avec le faux modèle local `rust/scripts/mock-model.js`.
- Ne jamais afficher une clé d'API.
- Modifier le code avec l'outil d'édition ou un script Python écrit dans un fichier, jamais par
  un heredoc ni par `sed -i`.
- Ne jamais lancer `claw /commande` depuis Git Bash (le chemin est transformé et part au modèle).
- Committer et pousser chaque lot vérifié avant d'en commencer un autre.

Vérification complète avant chaque commit (depuis `rust/`) :

```bash
cargo fmt --all
cargo clippy --workspace --all-targets                      # doit rester à zéro
CLAW_NO_USER_ENV=1 cargo test --workspace -- --test-threads=1
cargo build -p claw-cli
cd editors/vscode
CLAW_NO_USER_ENV=1 node test/client.test.js ../../target/debug/claw.exe
CLAW_NO_USER_ENV=1 node test/run-vscode.js ../../target/debug/claw.exe
```

S'y ajoutent la vérification Linux (`cargo clippy --target x86_64-unknown-linux-gnu` avec les
faux compilateurs C, voir la mémoire) et le contrôle des tirets longs.

## Méthode en mode Ultracode

- Petits lots : deux agents au plus par étude, sur un périmètre étroit. Deux études à cinq puis
  deux agents ont été perdues parce que la session s'est arrêtée avant leur rapport.
- Chaque agent écrit ses constats dans son fichier de [suivi/](suivi/) dès qu'ils sont confirmés.
  Si la session s'arrête, ce qui est écrit reste.
- Les agents lisent, ils ne modifient pas le code et ne lancent pas `cargo`.
- La session principale vérifie chaque constat, corrige, teste, committe, et met ce fichier à jour.

## Ce qui est fait

| Commit | Sujet |
|---|---|
| df5a5a1 | Signatures Gemini 3, commandes sans fin, lecture bornée, recherche sans dépendances, code des erreurs |
| 59c1dbc | Huit défauts de df5a5a1 corrigés : signatures par origine, quota du jour Google, commandes longues passées en arrière-plan, codages gardés, recherche qui ne cache plus de code |
| 2028cdd | Questions écrites en fin de réponse reposées avec le menu à choix |
| f637bf7 | Signalement d'un problème : méthode de débogage jointe, preuves demandées, images numérotées, commandes conseillées essayées, outil écrit comme une commande |
| 5260b7a | Outil `ViewPage` : texte affiché et console d'une page, dans un navigateur sans fenêtre |

État des tests au dernier commit : 581 tests Rust, analyse statique à zéro sous Windows et
Linux, client 28 sur 28, extension VS Code 18 sur 18.

## Études

| Étude | Fichier de constats | État |
|---|---|---|
| E1. Erreurs rendues au modèle par les outils : chaque message dit-il quoi faire ? | [suivi/e1-erreurs-outils.md](suivi/e1-erreurs-outils.md) | lancée le 11 octobre 2026 |
| E2. Sessions réelles de `test_claw` : où le modèle se trompe, ce que Claw a fait | [suivi/e2-sessions-reelles.md](suivi/e2-sessions-reelles.md) | lancée le 11 octobre 2026 |
| E3. Erreurs des fournisseurs et du flux : qui est prévenu, quelle reprise | [suivi/e3-erreurs-fournisseurs.md](suivi/e3-erreurs-fournisseurs.md) | à faire par la session principale |
| E4. Relecture indépendante des commits 59c1dbc à 5260b7a | à créer | à faire (deux tentatives interrompues) |

## À faire

Les identifiants servent à s'y retrouver d'une session à l'autre. « Preuve » dit d'où vient le
besoin.

| Id | Sujet | Preuve | État |
|---|---|---|---|
| A1 | Corriger les constats confirmés de E1 | étude E1 | en attente de l'étude |
| A2 | Corriger les constats confirmés de E2 | étude E2 | en attente de l'étude |
| A3 | Étude E3 puis corrections | à faire | à faire |
| A4 | Adresse locale donnée à l'utilisateur alors que rien n'écoute sur ce port : le dire au modèle avant que la réponse parte | session du 11 octobre : serveur arrêté par le modèle, puis « ouvrez http://localhost:8000 » | à faire |
| A5 | `ViewPage` : cliquer, saisir, déposer un fichier (protocole DevTools, client WebSocket à écrire) | session du 11 octobre : le défaut n'apparaît qu'après l'envoi d'une image | à faire |
| A6 | `grep_search` en mode `content` : `head_limit` et `offset` s'appliquent aussi à la liste des fichiers | relecture du 11 octobre | à faire |
| A7 | Relecture indépendante E4 | deux tentatives interrompues | à faire |
| A8 | Vérifier Gemini en conditions réelles (signatures renvoyées, schéma des outils) | demande une requête de génération : accord de l'utilisateur requis | bloqué par l'accord |
| A9 | Mesurer l'effet des contrôles sur de vrais modèles gratuits | demande de vraies requêtes : accord de l'utilisateur requis | bloqué par l'accord |

## Journal

### 10 et 11 octobre 2026

- Revue générale du code : cinq agents arrêtés par la limite de session, pistes reprises à la
  main (df5a5a1), puis relecture par un agent (huit constats, corrigés dans 59c1dbc).
- Session réelle `test_claw` (traducteur de mangas) étudiée message par message. Le modèle n'a
  lu qu'une image sur deux, a corrigé une cause supposée, a annoncé « résolu » sans rien
  observer, a réécrit quatre tests et a conseillé des commandes jamais lancées. Réponses de
  Claw : 2028cdd, f637bf7, 5260b7a.
- Ce fichier de suivi est créé, les études E1 et E2 sont lancées.
