# P1. Mode projet : créer un projet complet de A à Z

Demande de l'utilisateur, 14 octobre 2026 : « Qu'est ce qu'on peut ajouter comme aide pour
guider et supporter l'ia à l'orienter vers l'intelligence absolue et la création complète d'un
projet de A à Z » ; neuf aides proposées, réponse « intègre tous ».

## Principe

Un projet se crée en étapes que Claw connaît et contrôle. Le modèle écrit trois fichiers à la
racine du projet ; Claw les relit à chaque tour (et d'une session à l'autre) pour savoir où en
est le projet et ce qu'il doit vérifier :

- `SPEC.md`, le cahier des charges vivant : une ligne par exigence observable
  (`R1: ... Check: ...`) et par cas limite (`E1: ... -> résultat attendu`), puis les sections
  Questions et Hors périmètre.
- `PLAN.md`, les jalons : `## M1 - titre`, puis `Requirements: R1, E1`, `Verify: <commande>`,
  `Status: todo|done`. M1 est un squelette qui marche de bout en bout. Une section `## Demo`
  donne `Install:` et `Run:` (commandes depuis une copie vide du projet).
- `DECISIONS.md` : une ligne par choix (langage, bibliothèques, stockage, formats) et sa raison.

Claw garde son propre état dans `.claw/project.json` (mode actif, plan déjà relu par un autre
modèle, démonstration réussie, requêtes par jalon). Aide coupée par `CLAW_PROJECT_MODE=0`.

Activation : dossier sans fichier de code et demande de création (verbe et objet : « crée une
application... », au moins huit mots), ou « mode projet » écrit dans la demande. Le mode reste
actif pour ce dossier (état dans `.claw/project.json`) jusqu'à « sans mode projet ».

## Les neuf aides et leur lot

| Lot | Aide | Ce que fait Claw (déterministe sauf mention) |
|---|---|---|
| P1-1 | 1. Cahier des charges vivant | Guide de la méthode à l'activation ; `SPEC.md` relu après chaque écriture (exigences numérotées, doublons) ; ses exigences deviennent la référence du contrôle des exigences et de la relecture |
| P1-1 | 2. Tests d'acceptation d'abord | Guide : tests écrits avant le code, nommés d'après l'identifiant (`test_r3_...` ou commentaire `R3`) ; un jalon marqué fait dont une exigence n'a aucun test qui la nomme est signalé |
| P1-1 | 3. Plan en jalons | `PLAN.md` relu après chaque écriture (jalons, `Verify`, exigences sans jalon, identifiants inconnus) ; jalon en cours rappelé à chaque tour ; un jalon marqué fait voit sa commande `Verify` lancée par Claw, son échec renvoyé au modèle ; sans personne pour répondre, le modèle est renvoyé au jalon suivant |
| P1-1 | 8. Validation par l'utilisateur | Quand quelqu'un peut répondre : après `SPEC.md`, puis après chaque jalon, le modèle montre comment essayer et pose la question avec le menu à choix ; seul, il décide et note ses choix dans `DECISIONS.md` |
| P1-2 | 4. Sonde de l'environnement | Au premier tour de chaque session en mode projet : versions de Python, pytest, Node, npm, Git, Cargo, Go ; codage de la sortie de Python ; pièges connus (Python du Store, cp1252, modules retirés de Python 3.13, `tzdata` sous Windows) |
| P1-2 | 5. Journal des décisions | `DECISIONS.md` rejoint au premier tour de chaque session ; son absence après le plan est signalée |
| P1-3 | 6. Relecture d'architecture | Une fois `SPEC.md` et `PLAN.md` écrits, un autre modèle les relit avant le code (une ou deux requêtes) ; relu de nouveau seulement si des exigences ou des jalons changent |
| P1-4 | 7. Démonstration finale réelle | Tous les jalons faits : Claw copie le projet dans un dossier vide (fichiers suivis par Git, ou tout sauf les dossiers générés), y lance `Install` puis `Run` ; un échec repart au modèle ; README doit montrer la commande `Run` |
| P1-5 | 9. Budget de requêtes | Requêtes comptées par jalon ; quota gratuit restant lu chez `OpenRouter` au début du tour : s'il est bas, le modèle termine proprement le jalon ou s'arrête sur un état propre |
| P1-6 | Mesure | Scénario t8 : créer un projet complet dans un dossier vide, vérification cachée ; mesure avec et sans le mode projet |

## État

| Lot | État |
|---|---|
| P1-1 | fait le 14 octobre 2026 (commit « Mode projet, lot P1-1 ») |
| P1-2 à P1-6 | à faire |

### P1-1 : ce qui est fait

- Module `crates/runtime/src/project.rs` : lecture de `SPEC.md` (lignes `R1`, `E1` en liste, en
  gras, numérotées, en titre ou en tableau ; identifiants sans zéro en tête), de `PLAN.md`
  (titres `## M1`, `Milestone 2`, `Jalon 2`, case cochée ; `Verify:` en accents graves, en
  texte ou en bloc de code ; `Status:` fait ou à faire en anglais ou en français ; plages
  `R1-R3` ; section `## Demo` avec `Install:` et `Run:`), état `.claw/project.json`, phase du
  projet (cahier des charges, plan, jalon N, démonstration, terminé), guides, remarques.
- Activation : dossier sans code (documents, notes, images et dossiers cachés seulement) et
  demande de création (verbe ou besoin, objet, huit mots au moins ; « de A à Z », « from
  scratch » suffisent) ; « mode projet » l'ouvre, « sans mode projet » l'arrête, une question
  sur le mode ne l'ouvre pas.
- Boucle de conversation : guide au début de chaque tour (toute la méthode à l'ouverture,
  l'étape en cours ensuite, avec `DECISIONS.md`) ; après les appels d'une réponse, forme de
  `SPEC.md` et `PLAN.md` relue (une remarque par problème nouveau) et vérification immédiate
  d'un jalon marqué fait (commandes `Verify` lancées par l'outil `bash`, celles que le modèle
  vient de réussir comptent) ; en fin de tour, contrôle `project_gate` (jalon en échec rappelé,
  refait après un changement, trois renvois au plus ; exigences sans test ; cahier des charges
  d'abord ; seul, le plan jusqu'au bout, un renvoi par jalon ; avec quelqu'un, validation du
  cahier des charges) et `milestone_validation_gate` après la relecture (jalon terminé à faire
  essayer, question par le menu à choix) ; lignes du cahier des charges jointes au contrôle des
  exigences et à la relecture (`review_prompt_with`).
- Essais : sept essais unitaires (`project::tests`) et cinq de bout en bout avec le faux modèle
  scripté (seul jusqu'au bout du plan, avec l'utilisateur, jalon en échec puis corrigé,
  activation et arrêt, plan mal formé signalé une fois) ; essai du vrai binaire avec
  `mock-model.js` dans un dossier vide : guide, vérification du jalon, état enregistré.
- Incident : la première version de cet essai transmettait au binaire l'environnement du
  terminal, qui porte maintenant la vraie clé OpenRouter : la relecture de fin de tour est
  partie vers un vrai modèle gratuit (une requête). Les essais vident désormais les clés
  (`client.test.js`, `run-vscode.js`, scripts d'essai) ; règle notée dans la mémoire.
