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

1. Lire ce fichier en entier, puis `git log --oneline -15` et `git status`.
2. Aller à la section « Lots » : le premier lot dont l'état n'est pas « fait » est le suivant.
   Chaque identifiant (E1-07, E2-14...) renvoie à un constat détaillé dans [suivi/](suivi/),
   avec l'emplacement dans le code, la preuve et le remède proposé.
3. Vérifier chaque constat dans le code avant de le corriger (les numéros de ligne ont pu
   bouger), écrire le test, corriger, lancer la vérification complète, committer, pousser, puis
   mettre à jour ce fichier et l'état du constat dans son fichier d'étude, dans le même commit.

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
faux compilateurs C, voir la mémoire) et le contrôle des tirets longs. `CLAW_TEST_BROWSER=1`
active l'essai de `ViewPage` avec le vrai navigateur.

Outil pour lire une vraie session sans appeler de modèle :

```bash
python rust/scripts/session-trace.py SESSION.json --out trace.txt           # une ligne par bloc
python rust/scripts/session-trace.py SESSION.json --out blocs.txt --full 12 13
python rust/scripts/session-trace.py SESSION.json --out notes.txt --notes   # remarques de Claw
```

Les sessions réelles de l'utilisateur sont dans `D:\Mes_Projets\test_claw\.claw\sessions`.

## Méthode en mode Ultracode

- Petits lots : deux agents au plus par étude, sur un périmètre étroit. Deux études à cinq puis
  deux agents ont été perdues parce que la session s'est arrêtée avant leur rapport.
- Chaque agent écrit ses constats dans son fichier de [suivi/](suivi/) dès qu'ils sont confirmés.
  Si la session s'arrête, ce qui est écrit reste. Cette méthode a marché le 11 octobre 2026 :
  deux agents, 42 constats, rien de perdu.
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
| bf47311 | Ce fichier de suivi |
| 059aa2e | Études E1, E2 et E3 : 48 constats, plan en huit lots |
| (commit « Lot 1 ») | Lot 1 : code de sortie derrière `\| tail`, vérification juste, projet en sous-dossiers, reprises de l'API, conseils d'erreur |

État des tests au lot 1 : 587 tests Rust, analyse statique à zéro sous Windows et Linux,
client 28 sur 28, extension VS Code 18 sur 18.

## Études

| Étude | Fichier de constats | État |
|---|---|---|
| E1. Erreurs rendues au modèle par les outils : chaque message dit-il quoi faire ? | [suivi/e1-erreurs-outils.md](suivi/e1-erreurs-outils.md) | terminée le 11 octobre 2026 : 26 constats |
| E2. Sessions réelles de `test_claw` : où le modèle se trompe, ce que Claw a fait | [suivi/e2-sessions-reelles.md](suivi/e2-sessions-reelles.md) | terminée le 11 octobre 2026 : 16 constats |
| E3. Erreurs des fournisseurs et du flux : qui est prévenu, quelle reprise | [suivi/e3-erreurs-fournisseurs.md](suivi/e3-erreurs-fournisseurs.md) | terminée le 11 octobre 2026 : 6 constats |
| E4. Relecture indépendante des commits 59c1dbc à 5260b7a et des lots ci-dessous | à créer | à faire (deux tentatives interrompues) |

Études possibles ensuite : erreurs des outils de plugins et des serveurs MCP, outil
`PowerShell`, interface de la page (messages d'erreur montrés à l'utilisateur), mode équipe.

## Lots

Chaque lot est un commit. L'ordre suit le gain pour un petit modèle et la fréquence dans les
sessions réelles.

| Lot | Contenu | État |
|---|---|---|
| 1. Ce que Claw croit d'une commande | E2-14 (échec caché par `\| tail`), E2-11 (le mot `test` dans un chemin), E2-04 (vérification d'un projet en sous-dossiers), E2-15 (code des erreurs depuis un sous-dossier), E3-01, E3-02, E3-03 (reprises du client de l'API), E3-04 (contexte trop long), E3-06 (conseils d'erreur) | fait le 11 octobre 2026 (restent hors du lot : E2-14 secours (2), E2-15 dernier recours) |
| 2. Contrôles de fin de tour | E2-03 (le contrôle des exigences cite la demande), E2-08 (réponse faite de titres vides), E2-05 et E2-16 (images : défaut ou maquette, tri avant le guide), E2-01 (demande d'autorisation sans réponse) | à faire |
| 3. Fichiers et recherche | E1-19 (contenu abrégé écrit tel quel), E1-05 (dossier lu comme un fichier), E1-06 (motif non valide), E1-07 (filtres de `grep_search`), E1-10 et E2-12 (`glob_search`), E1-18 et E2-09 (`edit_file`), E1-21 (valeurs hors bornes), E1-22 (chemin introuvable) | à faire |
| 4. Arguments et outils inconnus | E1-01, E1-04, E1-11 (mise en forme des arguments d'après le schéma), E1-02 (outil inconnu), E1-03 (noms d'outils dans les sous-agents) | à faire |
| 5. Mémoire de la session | E2-06 (résumé de compaction qui n'en est pas un), E2-07 (résultats non allégés d'une session reprise), E3-05 (réponse coupée par la limite de sortie) | à faire |
| 6. Appels mal formés et sous-agents | E1-08 (bloc d'appel jeté en mode texte), E1-09 (réparation du JSON), E1-12 et E1-13 (sous-agents), E1-17 (lecture seule), E1-15 et E1-16 (web) | à faire |
| 7. Impact et cohérence de la réponse | E2-10 (réglage remplacé sans chercher qui en dépend), E2-02 (réponse qui cite ce qui n'existe pas), E2-13 (commande qui écrase un fichier), A4 (adresse locale où rien n'écoute) | à faire |
| 8. Le reste | E1-14, E1-20, E1-23 à E1-26 | à faire |

Hors lots :

| Id | Sujet | État |
|---|---|---|
| A5 | `ViewPage` : cliquer, saisir, déposer un fichier (protocole DevTools, client WebSocket à écrire) | à faire |
| A6 | `grep_search` en mode `content` : `head_limit` et `offset` s'appliquent aussi à la liste des fichiers | à faire, avec le lot 3 |
| A7 | Relecture indépendante (étude E4) | à faire après les lots 1 à 3 |
| A8 | Vérifier Gemini en conditions réelles (signatures renvoyées, schéma des outils) | bloqué : demande une requête de génération, accord de l'utilisateur requis |
| A9 | Mesurer l'effet des contrôles sur de vrais modèles gratuits | bloqué : demande de vraies requêtes, accord de l'utilisateur requis |
| A10 | `grep_search` accepte aussi les noms `A`, `B`, `C`, `n`, `i` (schémas réduits de E3-02) | à faire, avec le lot 4 |

## Journal

### 10 et 11 octobre 2026

- Revue générale du code : cinq agents arrêtés par la limite de session, pistes reprises à la
  main (df5a5a1), puis relecture par un agent (huit constats, corrigés dans 59c1dbc).
- Session réelle `test_claw` (traducteur de mangas) étudiée message par message. Le modèle n'a
  lu qu'une image sur deux, a corrigé une cause supposée, a annoncé « résolu » sans rien
  observer, a réécrit quatre tests et a conseillé des commandes jamais lancées. Réponses de
  Claw : 2028cdd, f637bf7, 5260b7a.
- Ce fichier de suivi est créé. Études E1 et E2 menées par deux agents (26 et 16 constats,
  écrits au fil de l'eau), étude E3 par la session principale (6 constats).
- Lot 1 fait. Ce que Claw croyait d'une commande était souvent faux : une compilation en
  échec derrière `| tail` passait pour réussie, `cd D:\...\test_claw` faisait de toute commande
  une vérification, un projet rangé en `backend/` et `frontend/` n'était jamais vérifié, et le
  code des erreurs citées depuis `frontend/` n'était pas trouvé. Côté fournisseurs : paramètre
  ou schéma refusé repris sans arrêter le tour, identifiants Mistral, contexte trop long mieux
  reconnu, conseil par code d'erreur.
