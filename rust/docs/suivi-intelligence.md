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

Depuis le 15 octobre 2026, ce dépôt est public et ses identifiants de commits ont changé
(adresse de l'auteur des deux premiers commits remplacée) : les identifiants cités avant cette
date se traduisent avec [suivi/correspondance-commits.md](suivi/correspondance-commits.md).
L'ancien dépôt privé, avec l'ancien historique, est archivé sous `abasse-ali/claw_code-prive`.

1. Lire ce fichier en entier, puis `git log --oneline -15` et `git status`.
2. Aller à la section « Lots » : le premier lot dont l'état n'est pas « fait » est le suivant.
   Depuis le 12 octobre 2026, la campagne d'essais réels (étude T1) passe avant les lots 6 à
   8 : reprendre au premier point ouvert de [suivi/t1-tests-reels.md](suivi/t1-tests-reels.md).
   Chaque identifiant (E1-07, E2-14...) renvoie à un constat détaillé dans [suivi/](suivi/),
   avec l'emplacement dans le code, la preuve et le remède proposé.
3. Vérifier chaque constat dans le code avant de le corriger (les numéros de ligne ont pu
   bouger), écrire le test, corriger, lancer la vérification complète, committer, pousser, puis
   mettre à jour ce fichier et l'état du constat dans son fichier d'étude, dans le même commit.

Depuis le 14 octobre 2026, le chantier du mode projet (créer un projet complet de A à Z,
neuf aides demandées par l'utilisateur) se suit dans [suivi/p1-mode-projet.md](suivi/p1-mode-projet.md) :
reprendre au premier lot qui n'est pas fait.

Depuis le 27 octobre 2026, le chantier « rivaliser avec Claude » (session #35, une appli de
budget : mesures justes, vrais bugs mesurés, le fond avant la forme, vitesse) se suit dans
[suivi/c1-rivaliser-claude.md](suivi/c1-rivaliser-claude.md), avec les décisions de
l'utilisateur (deux passages réels au plus, fusion directe dans `main`, gratuit uniquement,
barres gardées à 8) : les neuf lots sont faits et fusionnés dans `main` le 27 octobre 2026
(essai réel : session #36, 8/12 au banc t9 contre 4/12 pour #35, en 62 min au lieu de 109).
Second volet le 28 octobre 2026 (« c'est toujours moche ») : le meilleur modèle gratuit en
design (GLM-5.3, sinon Gemini 3.6 Flash ou MiniMax M3.1 Flash) écrit le plan et `base.css`, puis Nemotron 3 Ultra code tout,
jusqu'au bout ; lots 10 à 13 et la suite dans ce fichier. Troisième volet le même jour (« le site
fait vieux… plein d'incohérences et bugs graphiques ») : kit d'appli refait au niveau de l'appli de
Claude et écrit dans le projet, méthode, concepteur et juge propres aux applis perso, bugs
graphiques mesurés (lots 14 à 17) ; essai réel #39 : 11/12 au banc t9 au premier passage,
puis les défauts de tous les écrans mesurés avec leurs règles CSS (lot 18) avant le second.

Reprise automatique : arrêtée. Le 12 octobre 2026, l'utilisateur avait demandé que la
session reprenne d'elle-même (« continue ») à 6 h 22 puis toutes les 5 heures. Le 14 octobre
à 0 h 15, il a choisi d'arrêter : les tâches planifiées ont été supprimées. Une nouvelle
session ne les recrée pas ; le chantier reprend quand l'utilisateur le demande.

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
- Depuis le 12 octobre 2026, l'utilisateur demande des essais avec de vrais modèles gratuits
  pour améliorer Claw : peu d'essais, ciblés, par le banc `rust/scripts/real-tests/`, avec le
  nombre de requêtes noté. Tout le reste se teste toujours avec les faux modèles locaux.
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
active les essais de `ViewPage` et de `Screenshot` avec le vrai navigateur.

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
| 74bdcd1 | Lot 1 : code de sortie derrière `\| tail`, vérification juste, projet en sous-dossiers, reprises de l'API, conseils d'erreur |
| dfecc27 | Lot 2 : titres vides, images triées, demande citée au contrôle des exigences, demandes d'autorisation sans réponse ; E4A-01 et E4A-02 corrigés |
| 4358a70 | Code de sortie fidèle tube par tube, vérification du modèle comptée seulement si rien ne masque son échec, noms d'arguments réduits rendus à l'outil, règles `WebFetch` pour `ViewPage`, page entière et journal illisible pour `ViewPage`, aide d'erreur sur le code HTTP |
| fbc5369 | Serveurs arrêtés avec leurs petits-enfants sous Windows, serveur prêt malgré un délai long, bash sous Linux, pytest sans test, sous-projet à côté d'une racine vérifiable, code changé par une commande vérifié, `PowerShell` 5.1, navigateur qui refuse de démarrer, vLLM |
| 9ef1956 | Lot 3 : contenu abrégé refusé, dossier listé, `grep_search` et `glob_search` qui réparent et disent, `edit_file` qui joint ce qu'il faut pour réussir, BOM et CRLF gardés, chemin introuvable expliqué |
| 595bb90 | Lot 4 : arguments mis à la forme de l'outil et dits au modèle, outil inconnu expliqué, noms d'outils dans les sous-agents, `TodoWrite` tolérant |
| 695d03d | Lot 5 : résumé de compaction contrôlé et demandes notées mot pour mot par Claw, anciens résultats d'outils allégés à la réouverture, réponse coupée par la limite de sortie continuée et raccordée à l'affichage |

| b278ae6 à 53fecdb | Campagne T1 : banc d'essai réel (`rust/scripts/real-tests/`), série propre, constats T1-01 à T1-10 |
| a89ce3b | Lot T1 : garde des données réécrites, guide de débogage (hypothèse de l'utilisateur), fichier montré après une modification, essais regroupés, contexte Git borné, réglages sans secrets dans le prompt, Python du Microsoft Store |

| e3740fb, 83bb46a | Campagne T1 : mesure du lot T1 (34/36 contre 32/36, 74 requêtes contre 99), constat T1-13 |
| fcbbb34 | Compte rendu gardé quand la réponse à un contrôle n'en est qu'un accusé de réception (T1-13) ; vue de fichier repliée dans la page (T1-12) |
| a138b69, 83fb196 | Campagne T1 : scénario t4 (tâche longue sur plusieurs modules), essais, constats T1-14 à T1-16 |
| 93d40ae | Session enregistrée à chaque étape d'un tour, écriture atomique (T1-16) ; liste de tâches envoyée seule signalée et son résultat allégé (T1-14) ; remarque de regroupement plus tôt, redite, avec l'exemple des commandes du modèle (T1-15) ; fichier relu à l'identique gardé une fois et dit (T1-17) ; constats T1-18 et T1-19 |

| 806bf81 | Budget de requêtes par tour (T1-19) ; relecture qui aboutit sur un vrai diff : place, temps, raisonnement plafonné, seconde demande, échec expliqué (T1-20) ; relecture qui repart des phrases de la demande et règle de lecture dans le contrôle des exigences (T1-18, en partie) ; notices dans la sortie JSON (T1-21) ; `claw review-replay` |

| f2eac48 | Mesures de t4d (12/12, la relecture trouve un vrai défaut) et rejeux sur t4c ; vérification automatique qui trouve les tests pytest sans réglage (T1-22) ; second relecteur après une approbation sans analyse, verdict lu sans perdre les problèmes (T1-23) ; cas limites par genre de fonctionnalité dans le contrôle des exigences (T1-24 : 9/12 devient 12/12 au rejeu de t5) ; `checklist_replay.py`, `claw checklist-note` ; premier essai du scénario web t5 |

| 05d235f | t5b 12/12 (T1-24 confirmé sur un essai complet), petit modèle 10/12 ; vérification de t5 corrigée (une erreur en coûtait huit) ; rappel final de la forme au relecteur et double approbation nue dite à l'utilisateur (suite de T1-23) |

| 9c29c4a | Sans personne pour décider, une limite passagère du fournisseur est réessayée après une pause au lieu d'arrêter le tour (T1-25) ; cas « amont-limite » du faux modèle |

| d70da68 | Attente sans témoin allongée à une dizaine de minutes ; un tour en échec en mode JSON rend son erreur et ses notices ; t5 avec laguna : 12/12 |

| f744320 | Cas limites joints dès le début du tour et rappelés dans la demande de preuves (T1-26) ; piège précis des espaces insécables : t3 fait 4/4 pour la première fois (T1-04) ; t4 avec laguna : 12/12 en 16 requêtes |

| b2ebf80 | Chaque cas limite nomme son piège précis et l'essai à faire (T1-27) : au rejeu de t5, 12/12 en 14 requêtes au lieu de 22 |

| 5414b81 | Un fichier de données réécrit par une commande du modèle est vu par la garde des données (T1-11) |

| f1250cb | Scénario t6-conges (dates) : projet intact 1/12, référence 12/12 ; premier essai du plus puissant : 10/12 |

| 002c275 | Table des cas limites revue : faits vérifiés, mots-clés sans faux amis, quatorze genres nouveaux, résultat attendu de chaque essai, cas limites donnés au relecteur (T1-28) ; t6 passe de 10/12 à 12/12 (rejeu du contrôle et essai complet) |

| 866bc7e | Les essais d'une ligne partent ensemble (contrôle et guide) ; t6 refait à 12/12, t5 rejoué à 12/12 ; le petit modèle sur t6, arrêté, perd ses modifications par une commande (T1-29, ouvert) |

| 5795efe | Une commande qui supprime un fichier que le modèle avait modifié, ou le ramène à son état du début du tour, est signalée ; sa dernière version est gardée sous `.claw/kept/` (T1-29) |

| 8032009 | Scénario t7-ateliers (places limitées, doublons, données en JSON ; projet intact 1/12, référence 12/12) ; « inscription » à un atelier ne déclenche plus les comptes, les places et les doublons sont reconnus (T1-30) |

| ef6d54b | Script muet qui définit `main()` sans l'appeler, signalé (T1-31) ; une ligne de cas limite qui s'applique fait partie de la demande (T1-32) : t7 passe de 11/12, avec ou sans l'aide, à 12/12 |

| 9210cb1 | t6 : laguna 12/12 en 15 requêtes, le petit modèle 11/12 en 43 (contre un essai arrêté à 81 requêtes) ; l'essai `--help` de la ligne de commande attend aussi le format des arguments |

| 97fefbf | Garde des fichiers JSON de données : une migration faite à la main (champ ajouté à chaque entrée) est signalée, le code doit lire l'ancienne forme (T1-33) ; t7 répété : 3/12 avec et sans l'aide sans cette garde, 12/12 avec |

| 48d68e6 | Un ancien code qui saute un cas n'est pas une raison de le sauter : t7 rejoué au contrôle, 11/12 devient 12/12 |

| f273bc0 | t7 confirmé : 12/12 deux fois avec Claw à jour |

| 65a24fc | Une correction garde ce que renvoient les fonctions existantes (ligne des montants, étape 4 du débogueur) : t3 repasse de 2/4 à 4/4 (T1-34) |

| 9d58e46 | Relecture de secours par le modèle de la session quand aucun relecteur ne répond (T1-35) ; les phrases qui laissent une action à une personne sont citées au contrôle et à la relecture (T1-18) ; t2 8/8, t4 12/12 |

| 361f79d | La garde des tests ne vise plus les tests écrits par le modèle pendant le tour (T1-36) ; t1 6/6 et t5 12/12 avec Claw à jour : le banc entier tient |

| 53a7496 | Avec Claw au commit 361f79d : t6 12/12, t5 12/12, t4 12/12, sans remarque de la garde des tests (4 essais sur 22 la recevaient dans la journée) |

| f70d511 | Un petit projet est joint en vues au premier tour ; lire un fichier déjà en vue est signalé (T1-37) ; t6 : le plus puissant ne lit plus aucun fichier (11/12 en 29 requêtes) |

| ff3d899 | Mode projet : cahier des charges `SPEC.md`, plan en jalons `PLAN.md` vérifiés par Claw, tests d'acceptation nommés d'après les exigences, validation par l'utilisateur, plan suivi jusqu'au bout sans lui (P1-1) ; essais du client sans les vraies clés du terminal |

| 096f80d | Sonde de l'environnement et pièges connus au premier tour de la session, journal des décisions rappelé et réclamé (P1-2) ; relecture d'architecture par un autre modèle avant le code (P1-3) |

| dff13a1 | Démonstration finale réelle dans une copie vide du projet, README qui montre les mêmes commandes (P1-4) ; budget de requêtes par jalon et quota gratuit du jour dit au modèle et à l'utilisateur (P1-5) |

| 1188dfa | Scénario t8-materiel (créer un projet complet, 12 points, référence 12/12) ; démonstration lue en sous-titres et blocs de code, lancée en une seule session ; note cp1252 exacte (P1-6) |

| de3e5d3 | t8 avec et sans le mode projet : 11/12 des deux côtés, défauts différents ; ligne des doublons (comparer replié, afficher tel qu'écrit), « LGM » vaut approbation, pas de rappel du 40e appel en mode projet, vérifications du banc lisibles quel que soit le codage, `rescore.py` |

| 5d2bf93 | Mode projet avec le binaire à jour : t8 à 12/12 en 48 requêtes, relecture d'architecture et démonstration finale réelles ; les données que le programme écrit, non suivies par Git, restent hors de la copie vide |

| (commit « Bilan t8 ») | Comparaison à binaire égal : t8 à 12/12 avec (84 requêtes) et sans (41) le mode projet ; `> nul` de cmd réécrit `> /dev/null` dans Git Bash (un fichier `nul` restait dans le projet) |

| (commit « Captures d'écran », 15 octobre 2026) | Outil `Screenshot` : captures d'une page au téléphone et à l'ordinateur (navigateur piloté par le protocole DevTools, client WebSocket local), actions avant la capture (clic, saisie, défilement), défauts mesurés dans la page (contenu qui sort de l'écran, texte coupé, éléments superposés, `hidden` affiché, image absente, texte trop petit ou trop pâle, console) ; les captures suivent le dernier résultat d'outil de la réponse comme des images (décrites par le lecteur d'images pour un modèle qui ne les lit pas), quatre au plus dans la conversation ; ligne « Page layout » des cas limites ; `ViewPage` passe aussi par DevTools : sous Windows, Edge 155 se relance et `--dump-dom` ne rendait plus rien |

| (commit « Linux : arrêter une commande arrête aussi ses sous-processus », 16 octobre 2026) | Sous Ubuntu 24.04, `kill -KILL -<pid>` (procps 4) lit `-<pid>` comme un signal : rien n'était tué et la réponse était 0. Les descendants des commandes arrêtées (délai dépassé, tâche de fond, hook trop lent) survivaient, et les tests du crate runtime finissaient par geler la machine Linux de la CI sans laisser de journal. Forme `kill -s KILL -- -<pid>`, hooks dans leur propre groupe, essai sous Linux ; CI : durée maximale par job, limites de mémoire et de processus, tests Windows à la demande |

| (commit « Lecteur d'images : description en deux regards, LookAtImage », 16 octobre 2026) | Pour un modèle qui ne lit pas les images : description en six sections (nature, disposition avec positions approximatives, chaque élément avec texte exact, position, couleurs, taille et état, transcription intégrale, données, défauts), « unclear: » plutôt que deviner, 8 000 jetons ; relue contre l'image par un autre lecteur qui corrige et complète (une relecture coupée qui perd des sections est écartée) ; outil `LookAtImage` (identifiant de l'image et question précise) pour regarder à nouveau un détail. Essai réel : 3 requêtes gratuites, description de 5 000 caractères en 139 s, regard ciblé exact (texte et couleur d'un bloc de code coupé) |

État des tests après T1-33 : 664 tests Rust (le test de temps
`waiting_for_a_task_returns_once_it_is_done` peut échouer quand la machine est très chargée ;
il passe seul), analyse statique à zéro sous Windows et
Linux, client 28 sur 28, extension VS Code 18 sur 18 (lancée à T1-16 ; rien de l'interface n'a
changé depuis).

## Études

| Étude | Fichier de constats | État |
|---|---|---|
| E1. Erreurs rendues au modèle par les outils : chaque message dit-il quoi faire ? | [suivi/e1-erreurs-outils.md](suivi/e1-erreurs-outils.md) | terminée le 11 octobre 2026 : 26 constats |
| E2. Sessions réelles de `test_claw` : où le modèle se trompe, ce que Claw a fait | [suivi/e2-sessions-reelles.md](suivi/e2-sessions-reelles.md) | terminée le 11 octobre 2026 : 16 constats |
| E3. Erreurs des fournisseurs et du flux : qui est prévenu, quelle reprise | [suivi/e3-erreurs-fournisseurs.md](suivi/e3-erreurs-fournisseurs.md) | terminée le 11 octobre 2026 : 6 constats |
| E4. Relecture indépendante des commits 59c1dbc à 74bdcd1 (lot 1 compris) | [suivi/e4a-relecture-commandes.md](suivi/e4a-relecture-commandes.md), [suivi/e4b-relecture-fournisseurs.md](suivi/e4b-relecture-fournisseurs.md) | terminée le 11 octobre 2026 : 22 constats (E4A 12, E4B 10), deux agents. E4A-01 et E4A-02 corrigés dans le lot 2 ; les autres forment le lot 2b, sauf E4B-03, E4B-04 et E4B-07 rangés dans le lot 3. Les lots 2 à 3 seront relus ensuite (E5) |

| T1. Essais réels avec de vrais modèles gratuits : Claw amène-t-il le modèle à raisonner comme Opus et Fable ? | [suivi/t1-tests-reels.md](suivi/t1-tests-reels.md) | en cours depuis le 12 octobre 2026 : banc d'essai `rust/scripts/real-tests/` (trois scénarios notés par une vérification cachée), grille R1 à R8 |

Études possibles ensuite : erreurs des outils de plugins et des serveurs MCP, outil
`PowerShell`, interface de la page (messages d'erreur montrés à l'utilisateur), mode équipe.

## Lots

Chaque lot est un commit. L'ordre suit le gain pour un petit modèle et la fréquence dans les
sessions réelles.

| Lot | Contenu | État |
|---|---|---|
| 1. Ce que Claw croit d'une commande | E2-14 (échec caché par `\| tail`), E2-11 (le mot `test` dans un chemin), E2-04 (vérification d'un projet en sous-dossiers), E2-15 (code des erreurs depuis un sous-dossier), E3-01, E3-02, E3-03 (reprises du client de l'API), E3-04 (contexte trop long), E3-06 (conseils d'erreur) | fait le 11 octobre 2026 (restent hors du lot : E2-14 secours (2), E2-15 dernier recours) |
| 2. Contrôles de fin de tour | E2-03 (le contrôle des exigences cite la demande), E2-08 (réponse faite de titres vides), E2-05 et E2-16 (images : défaut ou maquette, tri avant le guide), E2-01 (demande d'autorisation sans réponse) | fait le 11 octobre 2026 |
| 2b. Corrections de la relecture E4 | Première partie : E4A-04 (`\|\| echo` après une vérification la fait compter faite), E4A-06 (`pipefail` tube par tube), E4B-01 (noms d'arguments réduits rendus à l'outil, avec A10), E4B-02 (aide d'erreur sur le code HTTP), E4B-05 (règle `WebFetch` qui couvre `ViewPage`), E4B-06 et E4B-08 (`ViewPage` : page entière, journal illisible). Seconde partie : E4A-03 (arrêt des serveurs sous Windows), E4A-07 (serveur prêt malgré un délai long), E4A-09 (bash plutôt que dash sous Linux), E4A-10 (pytest sans test), E4A-11 (`PowerShell` 5.1), E4A-12 (sous-projet à côté d'une racine vérifiable), E4B-09 (navigateur qui refuse de démarrer), E4B-10 (vLLM). Puis E4A-05 (serveur lancé avec `&` et sa sortie dans un fichier) et E4A-08 (code modifié par une commande) | fait le 11 octobre 2026, sauf E4A-05 (renvoyé au lot 7) |
| 3. Fichiers et recherche | E1-19 (contenu abrégé écrit tel quel), E1-05 (dossier lu comme un fichier), E1-06 (motif non valide), E1-07 (filtres de `grep_search`), E1-10 et E2-12 (`glob_search`), E1-18 et E2-09 (`edit_file`), E1-21 (valeurs hors bornes), E1-22 (chemin introuvable), E4B-03 (`multiline` de `grep_search`), E4B-04 (BOM et CRLF gardés par `write_file`), E4B-07 (`glob_search` dans un dossier `[v2]` ou sur un partage réseau) | fait le 11 octobre 2026 ; restent de E1-18 le texte d'avant une modification du même tour et la recherche dans d'autres fichiers, de E1-21 `Sleep` et `REPL` |
| 4. Arguments et outils inconnus | E1-01, E1-04, E1-11 (mise en forme des arguments d'après le schéma), E1-02 (outil inconnu), E1-03 (noms d'outils dans les sous-agents) | fait le 11 octobre 2026 |
| 5. Mémoire de la session | E2-06 (résumé de compaction qui n'en est pas un), E2-07 (résultats non allégés d'une session reprise), E3-05 (réponse coupée par la limite de sortie) | fait le 12 octobre 2026 |
| 6. Appels mal formés et sous-agents | E1-08 (bloc d'appel jeté en mode texte), E1-09 (réparation du JSON), E1-12 et E1-13 (sous-agents), E1-17 (lecture seule), E1-15 et E1-16 (web) | à faire |
| 7. Impact et cohérence de la réponse | E2-10 (réglage remplacé sans chercher qui en dépend), E2-02 (réponse qui cite ce qui n'existe pas), E2-13 (commande qui écrase un fichier), A4 (adresse locale où rien n'écoute), E4A-05 (serveur lancé avec `&` et sa sortie dans un fichier : à suivre comme une tâche, à arrêter à la fin de Claw) | à faire |
| 8. Le reste | E1-14, E1-20, E1-23 à E1-26 | à faire |

Hors lots :

| Id | Sujet | État |
|---|---|---|
| A5 | `ViewPage` : cliquer, saisir, déposer un fichier (protocole DevTools, client WebSocket à écrire) | en partie le 15 octobre 2026 : client WebSocket écrit, clic, saisie et défilement dans l'outil `Screenshot` ; déposer un fichier reste à faire |
| A6 | `grep_search` en mode `content` : `head_limit` et `offset` s'appliquent aussi à la liste des fichiers | fait (lot 3) |
| A7 | Relecture indépendante (étude E4) | à faire après les lots 1 à 3 |
| A8 | Vérifier Gemini en conditions réelles (signatures renvoyées, schéma des outils) | bloqué : demande une requête de génération, accord de l'utilisateur requis |
| A9 | Mesurer l'effet des contrôles sur de vrais modèles gratuits | bloqué : demande de vraies requêtes, accord de l'utilisateur requis |
| A10 | `grep_search` accepte aussi les noms `A`, `B`, `C`, `n`, `i` (schémas réduits de E3-02) | fait avec E4B-01 (lot 2b) |

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
- Étude E4 lancée (deux agents, relecture de 59c1dbc à 74bdcd1). Deux constats graves sur le
  lot 1 tout juste poussé : `pipefail` derrière `| head` faisait compter en échec des tests
  réussis (Python et les tests Rust ignorent SIGPIPE), et un fichier modifié dans
  `backend/tests/` faisait lancer pytest depuis `tests/`. Corrigés dans le lot 2 : une
  vérification suivie de filtres finit toujours (le reste de sa sortie est lu et jeté), et un
  dossier de tests n'est plus pris pour le projet.
- Lot 2 fait : titres vides renvoyés au travail, images triées (défaut vu par le lecteur ou
  maquette), demande citée mot pour mot au contrôle des exigences, demande d'autorisation sans
  réponse qui n'est plus reposée ni comptée comme un échec.
- Relecture E4 terminée : 22 constats. Lot 2b fait en deux commits : code de sortie fidèle tube
  par tube, vérification du modèle comptée seulement si rien ne masque son échec, noms
  d'arguments réduits rendus à l'outil, règles de permission de `WebFetch` étendues à
  `ViewPage`, serveurs arrêtés avec leurs petits-enfants sous Windows, pytest sans test,
  sous-projets, code changé par une commande vérifié. Reste E4A-05 (lot 7).
- Lot 3 fait : les outils de fichiers réparent ce qu'un petit modèle abîme (motif, filtres,
  guillemets échappés, `**.ts`) et le disent ; un contenu abrégé ne détruit plus un fichier ;
  un échec d'`edit_file` porte les vraies lignes du fichier, pour réussir sans relire.
- Lot 4 fait : un appel aux arguments mal nommés ou mal typés est réparé avant l'autorisation
  quand c'est sans ambiguïté, et le modèle l'apprend avec le résultat ; un outil inconnu reçoit
  les outils proches au lieu d'une demande d'autorisation ; `TodoWrite` ne se bloque plus.
- Processus de test laissés par les sessions des 8 et 9 octobre (faux serveurs `mock_*.py`,
  proxy, boucle d'attente) : leur arrêt a été refusé par le contrôle des autorisations ; à
  arrêter par l'utilisateur ou avec son accord (voir la mémoire du projet).

### 12 octobre 2026

- Lot 5 fait. Un résumé de compaction qui continue la conversation (cas réel de S1) est
  redemandé une fois avec une consigne plus nette, puis remplacé par le résumé de Claw ; après
  chaque résumé, Claw note lui-même les demandes de l'utilisateur mot pour mot (la première
  toujours, puis les plus récentes, 8 000 caractères ; une longue demande garde son début et
  sa fin) et les fichiers modifiés, repris d'une compaction à l'autre ; « continue » retrouve la
  demande poursuivie après une compaction. Les résultats d'outils d'une session enregistrée
  avant l'allègement sont allégés à sa réouverture. Une réponse en texte coupée par la limite
  de sortie est continuée (deux fois au plus par réponse), ne forme qu'un message, et
  l'affichage (page, panneau VS Code, terminal) raccorde la suite. Une session rouverte dans
  la page n'affiche plus son résumé comme un message de l'utilisateur.
- Relecture contradictoire du lot par deux agents avant le commit (méthode de l'étude E4 :
  lecture seule, constats écrits au fil de l'eau) : 15 constats, 8 sur la compaction et 7 sur
  la réponse coupée, tous corrigés et couverts par un test. Les plus sérieux : des faits placés
  dans les balises du résumé, qu'une demande citant `</summary>` abîmait ; une suite jointe
  après un bloc de raisonnement (modèles d'OpenRouter), qui coupait la réponse d'un saut de
  ligne ; une suite vide qui pouvait faire échouer le tour.
- Les 38 processus de test des 8 et 9 octobre (serveurs `mock_*.py`, `log_proxy.py`, boucle
  d'attente et leurs `bash`) sont arrêtés, à la demande de l'utilisateur ; son `uvicorn` reste.
- Nouvelle demande de l'utilisateur : tester Claw avec de vrais modèles gratuits et l'améliorer
  pour qu'il guide les modèles à raisonner comme Opus et Fable. Campagne T1 : banc d'essai
  réel, 99 requêtes pour la série propre. Le gratuit le plus puissant réussit presque tout ;
  un petit modèle réécrit de mémoire le fichier de données de l'utilisateur au lieu de
  corriger le script, et les deux gaspillent des requêtes à relire leurs propres
  modifications. Lot T1 fait en réponse, relu par deux agents (20 constats).
- Reprise automatique programmée à la demande de l'utilisateur (6 h 22 puis toutes les 5 h).
- Reprise de 6 h 22 : T1-13 et T1-12 corrigés. La suite est dans `suivi/t1-tests-reels.md`
  (scénarios plus durs d'abord).
- Même reprise, suite : scénario t4 (tâche longue). Le plus puissant le réussit en 31
  requêtes, dont 7 pour la seule liste de tâches et 14 commandes une par requête ; le petit
  modèle a dépensé 250 requêtes en 25 minutes sans finir, et sans laisser de trace. D'où
  T1-14 à T1-17 : la session s'écrit maintenant à chaque étape d'un tour (un tour interrompu
  se relit et se reprend ; vérifié en arrêtant `claw.exe` pendant une commande), la liste de
  tâches part avec l'appel suivant, la remarque de regroupement vient plus tôt et montre
  l'exemple, un fichier relu sans avoir changé n'est gardé qu'une fois. Second essai du petit
  modèle, lu pendant qu'il tournait : 34 requêtes, 8/12. Il a mal lu une phrase de la demande
  et ni son contrôle ni la relecture ne l'ont vu (T1-18) ; rien ne borne les requêtes d'un
  tour (T1-19). Ces deux points sont les suivants.
- Reprise de 11 h 22 : budget de 100 requêtes par tour (T1-19) et notices dans la sortie JSON
  (T1-21). En rejouant la relecture sur un essai enregistré (une requête, sans outil), on
  découvre qu'elle n'aboutissait jamais sur un vrai diff : les relecteurs gratuits raisonnent
  27 000 à 33 000 caractères et n'avaient pas la place de répondre (T1-20, corrigé). Une fois
  aboutie, elle approuve pourtant le travail qui a mal lu la demande ; une lecture indépendante
  par un autre modèle reprend la même erreur. Trois modèles sur quatre lisent « mis de côté
  pour le premier de la file : lui seul peut alors l'emprunter » comme un prêt d'office : la
  règle ajoutée au contrôle des exigences (une action laissée à une personne n'est pas faite
  automatiquement) reste à mesurer sur un essai réel (T1-18).
- Suite de la reprise de 11 h 22 : t4d fait 12/12, et la relecture, qui aboutit maintenant,
  y trouve un vrai défaut, corrigé avec un test. Un outil rejoue le contrôle des exigences au
  point exact où il était arrivé dans une session réelle (une à deux requêtes) : sur t4c, la
  règle de lecture ne change rien, le modèle traduit bien la phrase puis affirme que son
  prêt d'office la respecte. Ce qui trouve de vrais défauts, c'est un relecteur qui fait les
  deux étapes demandées ; une approbation nue fait donc consulter un second relecteur
  (T1-23). Corrigé aussi : la vérification automatique ne voyait pas les tests pytest sans
  réglage (T1-22). Premier essai du scénario web t5 : 9/12, le modèle oublie les accents, le
  `+` de l'URL et une recherche faite d'espaces. Claw rappelle maintenant, dans le contrôle des
  exigences, ce qu'un ingénieur expérimenté vérifie d'office pour le genre de fonctionnalité
  demandé (T1-24) : rejoué au même point, le même modèle passe de 9/12 à 12/12, puis 12/12 sur
  un essai complet ; le petit modèle fait 10/12. Un relecteur qui approuve sans analyse reçoit
  désormais, en fin de demande, le rappel de la forme exigée. Troisième modèle gratuit
  (laguna) : arrêté après trois réponses par une limite passagère de son fournisseur ; sans
  personne pour décider, Claw abandonnait au lieu d'attendre un peu (T1-25, corrigé). Refait :
  12/12, mais une limite plus longue a fini le tour en erreur ; l'attente est allongée et la
  sortie JSON garde l'erreur et les notices.
- Reprise de 16 h 22 : laguna fait aussi 12/12 sur t4, en 16 requêtes. Les cas limites sont
  maintenant joints dès le début du tour et rappelés dans la demande de preuves des problèmes
  signalés (T1-26). Sur t3, ni l'un ni l'autre ne suffisait tant que le rappel restait
  général ; nommer le piège précis (`replace(" ", "")` ne retire pas l'espace insécable d'Excel,
  essayer « 1\u00a0250,00 ») donne 4/4 pour la première fois (T1-04). Chaque cas limite a
  pris cette forme (T1-27) ; T1-11 est corrigé (garde des données après une commande).
  Nouveau scénario t6 (dates) : 10/12 au premier essai, le modèle laisse passer une période
  à l'envers et un solde négatif.
- Reprise de 21 h 22 (« ajoute ce qu'il y a à ajouter ») : la table des cas limites, revue par
  deux agents qui ont rejoué chaque fait, est corrigée (18 affirmations fausses ou
  incomplètes, des mots-clés qui se trompaient de genre) et passe de 9 à 23 genres (T1-28).
  Rejouée sur t6, elle ne suffisait pas : le modèle faisait les essais qui confirmaient son
  code. Avec le résultat attendu de chaque essai entre parenthèses, et la consigne qu'un autre
  résultat veut dire un code faux, même voulu, t6 fait 12/12 au rejeu comme à l'essai complet,
  mais en plus de requêtes. Le banc de rejeu réécrit maintenant les chemins de l'essai
  d'origine. Ensuite : t6 refait 12/12 avec la consigne de regrouper les essais (les essais
  du contrôle partent groupés, ceux du guide pas encore : consigne ajoutée au guide), t5
  rejoué 12/12. Le petit modèle sur t6 a été arrêté à 81 requêtes pour garder une réserve :
  il avait perdu deux fois ses modifications en supprimant puis restaurant `conges.py` par des
  commandes (T1-29). Corrigé ensuite : Claw garde la dernière version d'un fichier que le
  modèle avait modifié quand une commande le supprime ou le ramène en arrière, et le lui dit
  avec la commande qui le remet en place. Nouveau scénario t7 (ateliers à places limitées),
  préparé pour mesurer les genres nouveaux avec et sans l'aide ; sa demande, écrite comme on
  la dirait, montrait des mots-clés qui se trompaient de genre (T1-30, corrigé).
- Reprise du 13 octobre, 2 h 22 : t7 mesuré avec et sans l'aide des cas limites : 11/12 dans
  les deux cas. Avec l'aide, le modèle avait lu la ligne des doublons, vu lui-même que « zoé »
  passerait pour une autre personne, puis l'avait écartée : « la demande ne le dit pas ». Le
  contrôle dit maintenant qu'une ligne qui s'applique fait partie de la demande (T1-32) : 12/12
  au rejeu et à l'essai complet. Corrigé aussi : un script dont la commande ne fait plus rien
  parce que `main()` n'est jamais appelée est signalé au modèle (T1-31, vu sur le petit
  modèle dans t6). Puis t6 : laguna 12/12 en 15 requêtes ; le petit modèle 11/12 en 43
  requêtes, contre un essai arrêté à 81 la veille (il manque seulement le format des dates
  dans l'aide : l'essai `--help` l'attend maintenant). t7 répété : 3/12, avec et sans
  l'aide : le modèle migrait `donnees.json` à la main et son code plantait sur le fichier
  d'origine. La garde des données couvre maintenant les fichiers JSON de l'application
  (T1-33) : t7 refait, 12/12. Un essai de confirmation fait 11/12 : le modèle écartait la
  ligne des doublons parce que « le code existant ne le fait pas » ; complété (T1-32, suite),
  12/12 au rejeu.
- Reprise de 7 h 22 : t7 refait deux fois avec Claw à jour : 12/12 les deux fois (37 et 39
  requêtes). Avec les aides, t7 coûte une vingtaine de requêtes de plus que sans (17 à 19),
  pour un travail juste au lieu de 3/12 à 11/12. t3 refait avec la table revue : 2/4, le
  modèle avait fait renvoyer des `Decimal` à une fonction (la ligne des montants l'y poussait)
  et `json.dumps` plantait ; la ligne et le débogueur disent maintenant de garder ce que
  renvoient les fonctions existantes (T1-34) : t3 refait, 4/4. Le petit modèle sur t6 :
  10/12 en l'état, arrêté par la limite de temps. t2 refait : 8/8. t4 refait : 10/12, le
  prêt d'office revient (T1-18) et aucun relecteur ne répond à temps. Rejoué, le même modèle
  en relecteur lit lui aussi un prêt d'office, puis lit juste quand la phrase de l'utilisateur
  lui est citée ; au contrôle, l'agent, lui, défend son code même avec la phrase citée.
  D'où : relecture de secours par le modèle de la session (T1-35) et phrase citée à la
  relecture comme au contrôle. t4 refait : 12/12.
- Reprise de 20 h 55 : le quota du jour restait entier (416), t1 et t5 refaits : 6/6 et
  12/12. Le banc entier tient avec Claw à jour. La garde des tests se déclenchait souvent sur
  les tests neufs du modèle ; elle ne regarde plus que les lignes qui existaient au début du
  tour (T1-36). Les scripts du banc écrivent en UTF-8 (une réponse avec « → » faisait planter
  `review_replay.py`). Puis t6, t5 et t4 refaits avec ce Claw : 12/12 les trois fois, et
  plus aucune remarque de la garde des tests. Le petit modèle lisait les fichiers un par un
  (sept requêtes avant d'écrire) : un petit projet est maintenant joint en vues au premier
  tour (T1-37). Le plus puissant n'a plus rien lu sur t6 (11/12 en 29 requêtes) ; le petit
  modèle a moins lu, puis s'est perdu ailleurs (81 commandes isolées).
- 14 octobre, 0 h 15 : l'utilisateur arrête les reprises automatiques. Point d'arrêt : avec le
  plus puissant des modèles gratuits, les sept scénarios du banc ont tous atteint leur note
  maximale avec Claw à jour (le dernier essai de t6 : 11/12, consigne de l'aide précisée sans
  mesure). Ouverts : le petit modèle reste instable ; T1-18 en partie (l'agent défend son
  code, la relecture le rattrape) ; le format des arguments dans l'aide de la commande à
  mesurer.
- 14 octobre : l'utilisateur demande des aides pour mener un projet complet de A à Z ; neuf
  proposées (cahier des charges vivant, tests d'acceptation d'abord, plan en jalons, sonde de
  l'environnement, journal des décisions, relecture d'architecture, démonstration finale
  réelle, validation par l'utilisateur, budget de requêtes), réponse « intègre tous ». Plan en
  six lots dans [suivi/p1-mode-projet.md](suivi/p1-mode-projet.md). Lot P1-1 fait : mode
  projet, cahier des charges, plan en jalons vérifiés, tests d'acceptation, validation. Un
  essai du vrai binaire avec le faux modèle a laissé partir une relecture vers un vrai modèle
  gratuit (une requête) : le terminal porte maintenant la vraie clé ; les essais la vident.
  Lots P1-2 à P1-5 faits dans la nuit (sonde, décisions, relecture d'architecture, démonstration
  finale, budget). Scénario t8 (créer un projet complet) ; premiers essais réels à 2 h 15 :
  11/12 avec et sans le mode projet, 115 contre 87 requêtes ; le mode projet tient toutes les
  exigences écrites, le témoin oublie « à côté du programme » ; corrections tirées des deux
  essais (voir P1-6). Avec le binaire à jour : 12/12 en 48 requêtes (P2) ; puis, à binaire
  égal, 12/12 avec (84) et sans (41) le mode projet : sur ce scénario entièrement décrit, le mode
  projet ne change pas la note et coûte plus de requêtes ; sa démonstration finale a trouvé de
  vrais défauts que t8 ne note pas. Fichier `nul` créé par `> nul` dans Git Bash : réécrit.
