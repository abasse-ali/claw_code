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

Activation : dossier sans fichier de code et demande de création, ou « mode projet » écrit dans
la demande. Une demande de création a au moins huit mots, un verbe de création (« crée »,
« développe », « build »...) ou un besoin (« je veux »), suivi de ce qu'il faut créer, quel
qu'il soit (« un comparateur de prix »). Ne comptent pas : un texte ou un document seul (liste,
idées, plan, README, licence, `.gitignore`...), une question d'information (« Comment créer...
? »), un verbe à l'infinitif après « je veux » (« comprendre »), un verbe précédé d'un
déterminant, qui est alors un nom (« explique ce code »). Le mode reste
actif pour ce dossier (état dans `.claw/project.json`) jusqu'à « sans mode projet ». Seuls les
mots de l'utilisateur comptent : la demande jusqu'à sa première ligne `[Claw]` (ce qu'un moteur
y ajoute pour le modèle, comme celui de l'appli web, n'active ni n'arrête rien).

Reprise sans l'état (clone neuf, `.claw` effacé) : un dossier dont `SPEC.md` a des exigences et
`PLAN.md` des jalons reprend le mode projet tout seul ; son plan est tenu pour déjà relu (relu
de nouveau si des exigences ou des jalons changent). Un arrêt enregistré reste un arrêt.

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
| P1-1 | fait le 14 octobre 2026 (ff3d899) |
| P1-2 | fait le 14 octobre 2026 (096f80d) |
| P1-3 | fait le 14 octobre 2026 (096f80d) |
| P1-4 | fait le 14 octobre 2026 (dff13a1) |
| P1-5 | fait le 14 octobre 2026 (dff13a1) |
| P1-6 | fait le 14 octobre 2026 : scénario t8 (1188dfa), cinq essais réels ; à binaire égal, 12/12 avec (84 requêtes) et sans (41) le mode projet ; pistes de mesure plus parlantes notées plus bas |

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
### P1-2 : ce qui est fait

- Sonde (`project::probe_environment`, au premier tour de chaque session en mode projet, une
  fois par runtime) : Python (`python`, `py -3`, `python3`), sa version, pytest, le codage de
  sa sortie dans un tube, les fuseaux horaires de `zoneinfo`, en une seule commande Python ;
  Node, npm (`npm.cmd` sous Windows), Git, Cargo, Go, Java, en parallèle ; huit secondes au
  plus par commande. Le guide dit ce qui est installé, ce qui manque et les pièges : sortie en
  cp1252 dans le terminal de l'utilisateur (l'outil `bash` la masque en posant
  PYTHONIOENCODING), modules retirés de Python 3.12 et 3.13, `tzdata` absent, pytest à
  installer. Sur la machine de l'utilisateur : Python 3.13.14, pytest 9.1.1, cp1252, Node 24,
  npm 11, Git 2.52, Cargo 1.94, Java 1.8 ; Go absent.
- `DECISIONS.md` : sa fin est jointe au guide de chaque tour ; son absence est signalée une
  fois un jalon marqué fait, ou quand l'agent dit avoir fini.

### P1-3 : ce qui est fait

- Relecture d'architecture (`design_review`, après les appels d'une réponse) : dès que
  `SPEC.md` a des exigences et que `PLAN.md` a des jalons sans problème de forme, un autre
  modèle (`ConsultPurpose::Design`, mêmes délais que la relecture du code, même secours par
  le modèle de la session) lit la demande, les trois fichiers et la sonde, et cherche ce qui
  ferait échouer le projet : exigence implicite oubliée, exigence invérifiable, `Verify` qui ne
  prouve rien, M1 qui ne tourne pas de bout en bout, choix que la machine ne permet pas, démo
  qui échouerait dans une copie vide. Ses remarques vont au modèle avec la consigne de corriger
  les fichiers, ou de dire dans `DECISIONS.md` pourquoi il garde son choix. Une seule relecture
  par contenu (empreinte des exigences et des jalons dans `.claw/project.json`) ; sans
  relecteur, elle est retentée au tour suivant.

### P1-4 : ce qui est fait

- Démonstration finale (`final_demo`, contrôle de fin de tour, phase « tous les jalons faits ») :
  copie des fichiers que Git suit ou suivrait (`git ls-files --cached --others
  --exclude-standard`, sinon tout sauf les dossiers cachés, de dépendances et de sortie ; jamais
  `.claw`) dans un dossier neuf du dossier temporaire, puis lignes `Install` (cinq minutes au
  plus) et `Run` (deux minutes) lancées par l'outil `bash` dans cette copie, comme les tapera
  l'utilisateur. Un échec repart au modèle avec les causes habituelles (fichier hors du projet,
  paquet absent des dépendances, chemin relatif au dossier courant, commande fausse dans le
  README), trois fois au plus ; une réussite est notée pour ce plan (`demoPassed`), puis le
  README doit montrer les commandes `Run`. Sans section `## Demo`, elle est demandée. La copie
  est retirée après usage.
- Essai du vrai binaire avec le faux modèle : copie de quatre fichiers, `python biblio.py`
  lancé par Git Bash dans le dossier temporaire, réussite notée, copie retirée.

### P1-5 : ce qui est fait

- Validation par l'utilisateur : faite en P1-1 (cahier des charges, puis chaque jalon terminé).
- Budget : le coût de chaque jalon (requêtes depuis le début du tour ou la fin du jalon
  précédent, dans le tour où il se termine) est gardé dans `.claw/project.json`. Au début de
  chaque tour en mode projet, le client lit le quota gratuit du jour chez `OpenRouter`
  (`free_requests_left`, pour un modèle gratuit) ; s'il ne suffit pas pour les jalons restants
  au coût moyen mesuré (30 requêtes à défaut), le guide le dit au modèle (finir les jalons un à
  un, ou s'arrêter sur un état propre avec `PLAN.md` à jour) et une notice le dit à
  l'utilisateur. Quand la limite de requêtes du tour est atteinte, la note de conclusion
  demande en plus un `PLAN.md` qui dit où en est le jalon en cours.
- Limite connue : un jalon mené sur plusieurs tours n'est compté que pour son dernier tour.

### P1-6 : scénario t8 et mesure

- Scénario `t8-materiel` (banc `rust/scripts/real-tests/`) : créer, dans un dossier vide (un
  `.gitignore` seul), une application en ligne de commande de prêts de matériel (ajouter,
  prêter avec une date de retour, rendre, stock, retards ; données dans `materiel.json` à côté
  du programme ; refus clairs avec un code d'erreur ; tests ; README). Vérification cachée en
  12 points dans une copie sans données, comme chez un utilisateur qui vient d'installer :
  README, tests qui passent, ajouter et stock, données à côté du programme lancé d'un autre
  dossier, prêter, augmenter la quantité, plus d'exemplaire, matériel inconnu, rendre et
  retour inexistant, retards comptés le lendemain, saisies invalides sans plantage, sortie dans
  un tube sans UTF-8 forcé. Projet intact 0/12, référence 12/12 ; quatre variantes fautives de
  la référence perdent chacune le point attendu (coche hors cp1252, données dans le dossier
  courant, retard compté le jour même, prêt sans stock).
- Le premier essai réel a montré deux défauts, corrigés aussitôt (pour les essais suivants) :
  le modèle écrit la démonstration en sous-titres (`### Install`, `### Run`) suivis de blocs de
  code, que Claw ne lisait pas (il lit maintenant cette forme) ; il y met une activation
  d'environnement (`.venv/Scripts/activate`), qui ne vaut que dans la même session : les lignes
  de la démonstration partent maintenant en une seule commande, dans l'ordre, comme
  l'utilisateur qui les tape à la suite (commentaires de fin retirés). Le guide dit que Claw
  les lance dans une session du shell de l'outil `bash`.
- La note de la sonde sur le cp1252 était inexacte : une vraie console Windows affiche
  l'Unicode (PEP 528) ; c'est une sortie dans un tube ou un fichier (terminal de Git Bash,
  `> fichier.txt`, autre programme) qui passe en cp1252. La note le dit maintenant ainsi, et le
  point 12 de t8 accepte une sortie en UTF-8 ou en cp1252, pas un plantage.

#### Premiers essais réels de t8 (14 octobre, 2 h 15, quota du jour neuf)

Même modèle (`nvidia/nemotron-3-ultra-550b-a55b:free`), même binaire (avant la lecture des
démonstrations en sous-titres), limite de 150 requêtes par tour, une heure au plus. Les deux
essais ensemble ont pris 201 requêtes gratuites (relecteurs compris).

| Essai | Note | Requêtes du modèle | Durée | Point manqué |
|---|---|---|---|---|
| mode projet | 11/12 | 115 (110 appels d'outils) | 32 min | noms affichés en minuscules (« projecteur », « bob ») |
| sans mode projet (`CLAW_PROJECT_MODE=0`) | 11/12 | 87 (85 appels d'outils) | 22 min | `materiel.json` rangé dans le dossier courant au lieu d'être à côté du programme, pourtant écrit dans la demande |

- Avec le mode projet : `SPEC.md` (R1 à R7, E1 à E8), `PLAN.md` en six jalons au format
  attendu, `DECISIONS.md`, code en modules, 57 tests. Claw a vérifié chaque jalon marqué fait
  (coût mesuré : M1 35 requêtes, M2 9, M3 11, M4 6, M5 6, M6 14) ; la vérification de M6 a
  échoué une fois et le modèle l'a corrigée. La démonstration était écrite en sous-titres que
  ce binaire ne lisait pas : elle n'a pas tourné, et la relecture d'architecture non plus (elle
  attend un plan sans problème de forme). Ces deux points sont corrigés depuis.
- Le point manqué vient de la ligne des cas limites des doublons, qui disait de comparer les
  noms après `casefold()` : le modèle a gardé et affiché la forme repliée. La ligne dit
  maintenant que cette forme ne sert qu'à comparer, et que le nom se garde et s'affiche tel que
  l'utilisateur l'a écrit (essai : « Zoé » puis « zoé », une seule personne, toujours montrée
  « Zoé »).
- Sans le mode projet : un seul fichier de code, des tests, un README ; la vérification
  automatique (`python -m unittest discover`) n'a trouvé aucun test, le travail n'a donc pas
  été vérifié par Claw.
- Défauts du banc vus à cette occasion, corrigés : la vérification cachée de t8 plantait en
  imprimant son rapport vers une sortie en cp1252 (le piège même qu'elle mesure) : rapport en
  ASCII échappé, et `run.py` lance les vérifications avec `PYTHONIOENCODING=utf-8` ; les points
  10 et 12 de t8 échouaient pour la casse des noms, non pour ce qu'ils mesurent : la casse
  n'est plus exigée qu'au point 3 (nom tel qu'écrit dans `stock`) ; nouvel outil
  `rescore.py` qui rejoue la vérification d'un essai sans requête au modèle.
- Défauts de Claw vus à cette occasion, corrigés : un relecteur a conclu « VERDICT LGM »
  (faute de frappe), transmis comme une remarque : c'est maintenant une approbation ; le rappel
  du 40e appel d'outil (« si la tâche est finie, réponds maintenant ») tombait au milieu du
  jalon M2 sur six : il ne parle plus en mode projet, où le plan en jalons fait ce point.
- Lecture : un essai de chaque côté, même note, défauts différents. Le mode projet a tenu
  toutes les exigences écrites et produit un projet structuré et testé, pour 28 requêtes et
  10 minutes de plus ; son seul défaut venait d'une consigne de Claw, corrigée. Prochain essai :
  le mode projet avec le binaire à jour (démonstration finale et relecture d'architecture
  réelles).

#### Essai P2 : le mode projet avec le binaire à jour (14 octobre, 3 h 05)

Binaire au commit de3e5d3 (démonstration lue en sous-titres et lancée en une session, ligne
des doublons, pas de rappel du 40e appel). Même modèle, mêmes limites.

| Essai | Note | Requêtes du modèle | Durée |
|---|---|---|---|
| mode projet, binaire à jour | 12/12 | 48 (44 appels d'outils) | 17 min |

- Tout le circuit a tourné avec un vrai modèle : relecture d'architecture par laguna avant le
  code (six remarques : isoler les tests du vrai `materiel.json`, écriture atomique, `--help` et
  README dans un jalon...), quatre jalons vérifiés par Claw (11, 8, 7 et 5 requêtes),
  démonstration finale en échec deux fois puis réussie, contrôle des exigences, relecture sans
  remarque.
- Les deux échecs de la démonstration : la ligne `Install` et le README demandaient un
  `requirements.txt` qui n'existait pas (vrai défaut, corrigé par le modèle) ; puis « Bob a déjà
  un prêt » : la copie « vide » contenait le `materiel.json` rempli par les essais du modèle
  (défaut de Claw). Corrigé : les données que le programme écrit (données JSON, CSV, bases,
  journaux) qui ne sont pas suivies par Git restent hors de la copie, et la remarque d'échec le
  dit (« un programme crée ses données quand elles manquent ; s'il en faut une dès le départ,
  `git add` »).
- Lecture prudente : 12/12 en 48 requêtes contre 11/12 en 115 au premier essai du mode projet
  et 11/12 en 87 sans lui, mais un essai par côté, et le modèle a fait un plan plus court (quatre
  jalons au lieu de six). Deux essais suivent, avec et sans le mode projet, sur le même binaire.

#### Comparaison à binaire égal (14 octobre, 3 h 35, binaire 5d2bf93)

| Essai | Note | Requêtes du modèle | Durée | Remarques |
|---|---|---|---|---|
| mode projet | 12/12 | 84 (80 appels d'outils) | 28 min | relecture d'architecture sans remarque ; quatre jalons marqués faits d'un coup, chacun vérifié ; démonstration en échec une fois puis réussie, `materiel.json` laissé hors de la copie ; relecture finale : trois remarques |
| sans mode projet | 12/12 | 41 (38 appels d'outils) | 16 min | un fichier `nul` laissé dans le projet (`del ... > nul` lancé dans Git Bash) |

Bilan des cinq essais de t8 (même modèle) :

| Binaire | Mode projet | Sans mode projet |
|---|---|---|
| ff3d899 à 1188dfa (premiers essais) | 11/12, 115 requêtes | 11/12, 87 requêtes |
| de3e5d3 | 12/12, 48 requêtes | (pas d'essai) |
| 5d2bf93 | 12/12, 84 requêtes | 12/12, 41 requêtes |

- Sur t8, le plus puissant des modèles gratuits atteint 12/12 sans le mode projet dès que les
  autres aides de Claw sont à jour (ligne des doublons corrigée pendant la nuit). Le mode
  projet ne change pas la note de ce scénario et coûte en moyenne une fois et demie à deux
  fois plus de requêtes (82 contre 64 sur tous les essais, 84 contre 41 à binaire égal).
- Ce qu'il apporte et que t8 ne note pas : cahier des charges, plan, journal des décisions,
  tests nommés d'après les exigences, jalons vérifiés, relecture du plan, et surtout la
  démonstration dans une copie vide, qui a trouvé de vrais défauts (un `requirements.txt` promis
  et absent, des lignes d'installation fausses).
- t8 décrit l'interface en entier ; il éprouve peu ce qui fait la force du mode projet (une
  demande vague à préciser, un projet plus grand, un modèle plus faible). Pistes de mesure :
  une demande vague notée sur ce qu'elle implique, ou le petit modèle sur t8 (instable,
  coûteux en requêtes : à faire juste après une remise à zéro).
- Défaut de Claw vu dans le témoin, corrigé : `> nul`, `2>nul`, `< nul` (cmd) lancés dans Git
  Bash créaient un vrai fichier `nul` dans le projet, presque impossible à supprimer sous
  Windows ; l'outil `bash` les réécrit maintenant en `/dev/null`. La vérification de t8 saute
  les noms réservés de Windows (elle ne pouvait pas copier ce fichier).
- Requêtes gratuites de la nuit pour cette mesure : 201 (premiers essais), 49 (P2), 229 (les
  deux essais à binaire égal, relecteurs compris) ; il en restait plus de 500 à 4 h.

- Incident : la première version de cet essai transmettait au binaire l'environnement du
  terminal, qui porte maintenant la vraie clé OpenRouter : la relecture de fin de tour est
  partie vers un vrai modèle gratuit (une requête). Les essais vident désormais les clés
  (`client.test.js`, `run-vscode.js`, scripts d'essai) ; règle notée dans la mémoire.
