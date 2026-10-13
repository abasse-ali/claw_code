# Campagne T1 : essais réels avec de vrais modèles gratuits

Demande de l'utilisateur, le 12 octobre 2026 : « Fais des vrais tests sur Claw avec de vrai
models, et améliore le, qu'il prenne exemple sur comment les models Opus et Fable
réfléchissent. Fais toujours en sorte qu'on puisse reprendre sur une autre session. »

## Méthode

- Banc d'essai : `rust/scripts/real-tests/` (voir son README). Sept scénarios (t1 à t7, dont
  un projet web, des dates et des places limitées), chacun avec une vérification cachée qui
  note le résultat ; les essais tournent hors du dépôt, dans
  `D:\Mes_Projets\claw-real-tests`, une ligne par essai dans `results.jsonl`.
- Modèle : le réglage de l'utilisateur (`free`, le gratuit le plus puissant du moment), sans
  bascule (`CLAW_FALLBACK=0`). D'autres modèles gratuits ensuite, pour vérifier que les
  améliorations ne dépendent pas d'un seul modèle.
- Chaque essai est lu dans sa session (`python rust/scripts/session-trace.py`) et comparé à ce
  qu'aurait fait un modèle fort (grille ci-dessous). Chaque écart devient un constat T1-nn,
  avec sa preuve (session, message) et un remède dans Claw.
- Quota : chaque étape de l'agent coûte une requête ; le nombre est noté pour chaque essai.

## Grille : comment raisonnent Opus et Fable sur une tâche de code

Ce que Claw doit amener un modèle gratuit à faire, au bon moment, par des remarques courtes
plutôt que par une longue consigne :

- R1. Comprendre avant d'agir : l'objectif en une phrase, chaque exigence (dite ou impliquée),
  ce qui prouvera que c'est fait. Une affirmation de l'utilisateur (« le fichier est
  corrompu ») est une hypothèse à vérifier, pas une consigne.
- R2. Des faits, pas des souvenirs : reproduire (lancer les tests, le script), lire le code en
  cause, plusieurs lectures indépendantes dans la même réponse.
- R3. Hypothèses et preuves : pour un échec, plusieurs causes possibles, un test qui les
  départage, la cause dite en une phrase avec sa preuve ; chercher s'il y en a plusieurs.
- R4. Le plus petit changement qui supprime la cause, dans le style du code autour ; ne pas
  toucher aux données de l'utilisateur ni affaiblir un test pour le faire passer.
- R5. Vérifier le comportement, pas seulement que ça tourne : relancer les tests, essayer la
  fonction avec de vraies entrées, cas limites compris (vide, accents, absent, erreur).
- R6. Se relire comme un relecteur : chaque exigence a-t-elle sa preuve ? que pourrait-il
  rester de faux ?
- R7. Rendre compte honnêtement : la cause et sa preuve, ce qui a été fait et vérifié, ce qui
  ne l'a pas été ; corriger poliment une idée fausse de l'utilisateur.
- R8. Persévérer avec méthode : un échec se lit et s'explique avant d'essayer autre chose ; ne
  pas répéter une action qui vient d'échouer.

## Essais

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 01:02 | t1-inventaire (première version) | free = nemotron-3-ultra-550b | base | 5/5 | 11 | 127 s | une seule cause en fait : 15 x 0.1 vaut exactement 1.5 ; scénario corrigé ensuite (test sur 0.3) |
| 12/10 01:05 | t2-notes | free = nemotron-3-ultra-550b | base | 8/8 | 14 | 103 s | exigences implicites toutes tenues ; réponse finale devenue un tableau d'audit (T1-02) |
| 12/10 01:06 | t3-rapport | free = nemotron-3-ultra-550b | base | 3/4 | 6 | 63 s | code corrigé, données intactes ; espaces insécables oubliées (T1-04) ; l'idée « fichier corrompu » jamais démentie (T1-03) |
| 12/10 01:09 | t1-inventaire (deux causes) | free = nemotron-3-ultra-550b | base2 | 6/6 | 10 | | **faussé** : le diff Git du dépôt, dans le prompt, contenait la correction du scénario (T1-05) |
| 12/10 01:11 | t1-inventaire (deux causes) | cohere/north-mini-code | north | 6/6 | 20 | | **faussé** : le modèle a lu `docs/suivi/t1-tests-reels.md` en remontant les dossiers, après l'avoir vu dans le diff Git |

Les essais du 12 octobre avant 01:15 tournaient dans `rust/target/real-tests/`, donc dans le
dépôt : les trois premiers sont sans doute propres (aucune modification non commitée ne
parlait des pièges), les suivants non. Depuis, `run.py` fait tourner les essais hors du dépôt
(`D:\Mes_Projets\claw-real-tests`, à côté du dépôt ; pas `%LOCALAPPDATA%`, que le Python du
Microsoft Store virtualise), chaque projet dans son propre dépôt Git, et refuse un dossier qui
a des instructions ou un dépôt au-dessus de lui. Tous les essais sont à refaire dans ces
conditions.

Série propre (hors du dépôt), Claw au commit 695d03d :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 01:16 | t1-inventaire | free = nemotron-3-ultra-550b | clean | 6/6 | 8 | 117 s | deux causes trouvées et prouvées, arrondi monétaire dans le code |
| 12/10 01:18 | t2-notes | free = nemotron-3-ultra-550b | clean | 8/8 | 20 | 181 s | 3 relectures de `notes.py` après ses propres modifications, 7 essais en ligne de commande un par requête (T1-09, T1-10) |
| 12/10 01:21 | t3-rapport | free = nemotron-3-ultra-550b | clean | 3/4 | 6 | 27 s | code corrigé, données intactes ; espaces insécables oubliées ; ne dit pas que le fichier n'était pas corrompu (T1-03, T1-04) |
| 12/10 01:21 | t1-inventaire | cohere/north-mini-code | north | 6/6 | 20 | 113 s | a d'abord affaibli le test (`math.isclose`) ; la remarque « test réécrit après un échec » l'a fait revenir au code : aide efficace. 5 relectures, 4 micro-essais (T1-09, T1-10) |
| 12/10 01:23 | t2-notes | cohere/north-mini-code | north | 8/8 | 36 | 231 s | 13 lectures de `notes.py` (par tranches, après chaque modification), 6 requêtes perdues dans `/tmp` avec le Python du Microsoft Store (T1-07), 6 essais un par requête |
| 12/10 01:27 | t3-rapport | cohere/north-mini-code | north | 1/4 | 9 | 33 s | a réécrit `ventes.csv` en entier, de mémoire, sans reproduire ni toucher au script (T1-08) |

Total de la série : 99 requêtes gratuites.

Série de mesure après le lot T1 (Claw au commit a89ce3b), mêmes conditions :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 02:20 | t1-inventaire | free = nemotron-3-ultra-550b | lotT1 | 6/6 | 10 | 123 s | seconde cause réglée dans le test (`pytest.approx`), dit et justifié après la garde des tests ; relecture par un autre modèle déclenchée (plus de lignes changées) ; réponse finale réduite à un accusé de réception (T1-13) |
| 12/10 02:22 | t2-notes | free = nemotron-3-ultra-550b | lotT1 | 8/8 | 10 | 123 s | 4 modifications sans aucune relecture, 3 commandes d'essai au lieu de 9 (remarque de regroupement reçue) |
| 12/10 02:24 | t3-rapport | free = nemotron-3-ultra-550b | lotT1 | 3/4 | 7 | 151 s | la réponse dit « le fichier n'est pas corrompu, votre hypothèse était incorrecte » et « ventes.csv n'a pas été modifié » ; correctif essayé sur cinq valeurs hors de l'exemple ; espaces insécables toujours oubliées |
| 12/10 02:27 | t1-inventaire | cohere/north-mini-code | lotT1-north | 6/6 | 18 | 137 s | 3 lectures au lieu de 7 |
| 12/10 02:29 | t2-notes | cohere/north-mini-code | lotT1-north | 8/8 | 19 | 141 s | 4 lectures de `notes.py` au lieu de 13 ; pas de détour par `/tmp` cette fois |
| 12/10 02:31 | t3-rapport | cohere/north-mini-code | lotT1-north | 3/4 | 10 | 46 s | `ventes.csv` intact, script corrigé, hypothèse de l'utilisateur démentie dans la réponse |

Scénario t4 (tâche longue, plusieurs modules), Claw au commit fcbbb34 :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 07:00 | t4-bibliotheque | free = nemotron-3-ultra-550b | t4 | 12/12 | 31 | 379 s | liste de tâches tenue, tout est fait ; 7 requêtes pour la seule mise à jour de la liste (T1-14), 14 commandes une par requête (T1-15) |
| 12/10 07:06 | t4-bibliotheque | cohere/north-mini-code | t4-north | 8/12 | 250 | 1 500 s | arrêté par la limite de 25 minutes ; le livre n'est pas mis de côté au retour ; plus aucun fichier modifié après la 6e minute, donc environ 200 requêtes sans effet (T1-17) ; aucune trace : la session n'est enregistrée qu'en fin de tour (T1-16) |
| 12/10 08:02 | t4-bibliotheque | cohere/north-mini-code | t4b-north | 8/12 | 35 | 349 s | Claw avec T1-14 à T1-16 ; terminé normalement, la dérive du premier essai ne s'est pas reproduite ; relit quatre fichiers juste avant de les modifier (T1-17) ; prête le livre d'office au premier de la file au lieu de le mettre de côté, et son contrôle des exigences ne le voit pas (T1-18) ; la relecture par un autre modèle n'a vraisemblablement pas abouti (T1-20) ; trois remarques de regroupement reçues, sans effet |
| 12/10 08:28 | t4-bibliotheque | free = nemotron-3-ultra-550b | t4c | 9/12 | 30 | 455 s | même Claw ; une seule mise à jour de la liste envoyée seule (contre sept) : après la remarque, les cinq suivantes partent avec une modification (T1-14) ; après les remarques des 6e et 9e réponses isolées, il enchaîne deux puis quatre commandes par appel, toujours un appel par réponse (T1-15) ; a mal lu la même phrase que le petit modèle : prêt d'office au premier de la file (T1-18) |
| 12/10 12:34 | t4-bibliotheque | free = nemotron-3-ultra-550b | t4d | 12/12 | 29 | 779 s | Claw au commit 806bf81 ; bonne lecture de « mis de côté » dès son plan (deux essais sur trois pour ce modèle) ; la relecture aboutit (dots, les deux étapes suivies) et trouve un vrai défaut, l'emprunteur qui peut réserver son propre livre : corrigé avec un test, 12/12 au lieu de 11/12 ; liste de tâches toujours jointe à la modification suivante ; la vérification automatique de Claw lance `unittest`, qui ne trouve pas les tests pytest (T1-22) |
| 12/10 16:40 | t4-bibliotheque | poolside/laguna-s-2.1 (troisième modèle) | t4-laguna | 12/12 | 16 | 1 053 s | Claw au commit d70da68 ; bonne lecture de « mis de côté » ; deux fois moins de requêtes que le plus puissant ; une limite du fournisseur, attendue une fois (T1-25, vu en réel) ; la relecture (dots) relève une faute dans un message du guichet |

Rejeux sur le travail enregistré de t4c (`review_replay.py`, `checklist_replay.py` : le
contrôle des exigences renvoyé au point exact où Claw l'avait envoyé, message 61 ; une à deux
requêtes chacun) :

| Date | Rejeu | Relecteur ou modèle | Résultat |
|---|---|---|---|
| 12/10 12:15 | relecture, consignes actuelles | laguna (choisi par Claw) | « VERDICT / LGTM » en 8 s, sans l'analyse demandée : approuve le prêt d'office |
| 12/10 12:51 | contrôle des exigences, formulation de 93d40ae | free | 9/12 : il confirme sa lecture, comme dans l'essai |
| 12/10 12:52 | contrôle des exigences, formulation actuelle | free | 9/12 : il traduit bien la phrase (« set aside for the first in line: only they can then borrow it »), puis écrit que son code la respecte et que ses tests « valident le passage automatique », alors que sa sortie montre « emprunté par Bilal » |
| 12/10 12:54 | relecture, consignes actuelles | dots (imposé) | suit les deux étapes et trouve un vrai bogue (le premier de la file emprunte un livre pas encore rendu), sans relever le prêt d'office ; finit sa liste de problèmes par « LGTM » (T1-23) |

Scénario t5 (projet web : serveur Python, API JSON, page), Claw au commit 806bf81 et T1-22 :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 13:04 | t5-recettes-web | free = nemotron-3-ultra-550b | t5 | 9/12 | 16 | 351 s | lit tout le projet d'un coup, liste de tâches jointe aux modifications ; la vérification automatique lance pytest (T1-22, vu en réel) ; oublie les accents, le `+` de l'URL (requête lue à la main avec `unquote`) et une recherche faite d'espaces ; la relecture (laguna, avec analyse) approuve sans les voir (T1-24) |
| 12/10 13:12 | t5-recettes-web | cohere/north-mini-code | t5-north | - | - | - | interrompu à la deuxième minute par l'arrêt de la session de travail, sans note ; sa session, enregistrée au fil du tour, garde 49 messages lisibles : première confirmation réelle de T1-16 |
| 12/10 14:44 | t5-recettes-web | free = nemotron-3-ultra-550b | t5b | 12/12 | 29 | 676 s | Claw au commit f2eac48 (T1-24) : au contrôle des exigences, il vérifie les cas limites de la recherche et corrige accents, espaces et décodage de l'URL, avec des tests ; deux relecteurs répondent « VERDICT LGTM » sans analyse (le second par T1-23) |
| 12/10 14:56 | t5-recettes-web | cohere/north-mini-code | t5-north2 | 10/12 | 49 | 604 s | même Claw ; accents, espaces et décodage justes ; un ingrédient doit égaler le mot cherché (« pomme » ne trouve pas « pommes de terre ») et plusieurs mots ne trouvent rien ; 16 lectures de fichiers, 6 retirées du contexte et 2 remarques (T1-17) ; la relecture (nemotron-ultra) relève deux affirmations fausses de sa réponse (un test qui n'existe pas, un compte de tests) |
| 12/10 15:20 | t5-recettes-web | poolside/laguna-s-2.1 (troisième modèle) | t5-laguna | non noté | 3 | 144 s | bonne lecture de tout le projet, plan clair, une modification ; puis « laguna est temporairement limité en amont, réessayez bientôt » (429) : trois essais en 4 s et le tour s'arrête (T1-25). La session, enregistrée au fil du tour, garde ses 14 messages |
| 12/10 15:31 | t5-recettes-web | poolside/laguna-s-2.1 | t5-laguna2 | 12/12 | 26 | 1 124 s | Claw au commit 9c29c4a (T1-25) : le travail est complet, et au contrôle des exigences il ajoute l'insensibilité aux accents (T1-24) ; puis une limite du fournisseur dure plus que les quatre minutes d'attente prévues et le tour finit en erreur, sans JSON ni notices (suite de T1-25) |

Scénario t6 (dates : jours ouvrés d'un congé, dates à la française, fériés, solde ; projet
intact 1/12, solution de référence 12/12), Claw au commit 5414b81 :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 12/10 20:26 | t6-conges | free = nemotron-3-ultra-550b | t6 | 10/12 | 19 | 446 s | dates à la française, bornes comprises, week-ends, fériés (un samedi compté une fois) et année bissextile justes ; une période dont la fin précède le début est comptée au lieu d'être refusée, et un solde peut devenir négatif (Bilal à -3) ; la relecture (dots) ne voit rien |
| 12/10 22:28 | t6-conges | free = nemotron-3-ultra-550b | t6-t28 | 12/12 | 32 | 664 s | Claw avec la table des cas limites revue (T1-28) : la période à l'envers est refusée, un solde insuffisant aussi, avec ce qui reste ; la relecture (laguna) reprend les cas limites dans son étape 1 et trouve l'aide `--help` manquante |
| 12/10 22:50 | t6-conges | free = nemotron-3-ultra-550b | t6-t28c | 12/12 | 32 | 482 s | avec la consigne de regrouper les essais d'une ligne au contrôle : après le contrôle, quatre commandes groupées au lieu d'une par essai ; avant lui, cinq essais isolés venus du guide du début de tour (la consigne y a été ajoutée ensuite) et cinq mises à jour seules de la liste de tâches malgré trois remarques (T1-14) |
| 13/10 08:22 | t6-conges | north-mini-code (petit modèle) | t6-north3 | 10/12 en l'état | 48 | 1 200 s | Claw au commit f273bc0 : arrêté par la limite de 20 minutes, travail en cours (ses tests échouaient encore, l'aide sans le format des dates) ; ce modèle reste très variable (11/12 en 43 requêtes à l'essai précédent) |
| 13/10 03:17 | t6-conges | poolside/laguna-s-2.1 (troisième modèle) | t6-laguna | 12/12 | 15 | 1 548 s | Claw avec T1-29 à T1-32 : tout juste en 15 requêtes ; lent (une limite 429 du fournisseur, une réflexion qui prend toute la réponse : niveau ramené à « high ») ; la relecture (nemotron) : rien à corriger |
| 13/10 03:43 | t6-conges | north-mini-code (petit modèle) | t6-north2 | 11/12 | 43 | 971 s | même Claw : tout juste sauf l'aide, qui cite `jours` sans le format des dates (l'essai `--help` de la ligne de commande n'attendait que la commande : il attend maintenant aussi le format) ; ni garde des modifications perdues ni script muet cette fois ; un modèle aussi variable demande plusieurs essais avant de conclure |
| 12/10 23:04 | t6-conges | north-mini-code (petit modèle) | t6-north | arrêté, 2/12 en l'état | 81 | 659 s | arrêté pour garder une réserve de requêtes gratuites (38 jusqu'à 2 h) ; une trentaine de requêtes perdues à l'édition : il croit voir un bloc en double, `edit_file` refuse (le bloc n'existe pas), il supprime `conges.py` par une commande, le restaure avec `git checkout` et perd ses modifications, deux fois (T1-29) ; ses essais partent ensuite un par requête |

Rejeux du contrôle des exigences de t6 (message 35). Mesure non indépendante : les lignes des
dates et des soldes ont été écrites après avoir vu les deux erreurs de t6.

| Date | Formulation | Modèle | Résultat |
|---|---|---|---|
| 12/10 20:37 | celle de 5414b81 | free | 10/12, 9 requêtes |
| 12/10 22:09 | table revue (T1-28) : faits vérifiés, soldes, période à l'envers | free | 10/12, 11 requêtes : il fait les essais qui confirment son code (31/04, 29/02, accents), garde « 5 (échange auto) » pour une période à l'envers et n'essaie pas le solde |
| 12/10 22:20 | et le résultat attendu de chaque essai entre parenthèses ; un autre résultat veut dire un code faux, même voulu | free | 12/12, 40 requêtes : il refuse la période à l'envers « au lieu d'inverser silencieusement », refuse un solde insuffisant en disant ce qui reste et corrige son propre test qui exigeait l'échange ; un essai par requête |

Deux défauts du banc vus pendant ces rejeux, corrigés : la session rejouée gardait les chemins
de l'essai d'origine (le modèle a lancé des essais dans l'ancien projet), et le rejeu joignait
une seconde fois le guide des cas limites. Les rejeux suivants ne sont donc pas tout à fait
comparables aux précédents.

Scénario t7 (places limitées, doublons, données en JSON ; projet intact 1/12, solution de
référence 12/12), Claw au commit 8032009 puis avec T1-32 :

| Date | Scénario | Modèle | Étiquette | Score | Requêtes | Durée | Remarques |
|---|---|---|---|---|---|---|---|
| 13/10 02:53 | t7-ateliers | free = nemotron-3-ultra-550b | t7-sans (`CLAW_EDGE_CASES=0`) | 11/12 | 17 | 312 s | témoin sans les cas limites : tout juste, sauf « zoé » et «  Zoé  » inscrits en plus de « Zoé » |
| 13/10 02:58 | t7-ateliers | free | t7-avec | 11/12 | 21 | 245 s | même point manqué. Au contrôle, le modèle lit la ligne des doublons et écrit : « "Zoé", " zoé ", and decomposed "zoé" would count as three different people. The guide suggests normalizing, but the request doesn't explicitly ask for it » (T1-32) ; la relecture (laguna) n'y voit rien |
| 13/10 04:09 | t7-ateliers | free | t7-sans2 (`CLAW_EDGE_CASES=0`) | 3/12 | 19 | 140 s | témoin répété : le modèle ajoute `"inscrits": []` à chaque atelier de `donnees.json` (migration à la main) et écrit un code qui ne lit plus l'ancienne forme ; sur le fichier d'origine, `liste` plante (KeyError) et presque tout échoue (T1-33) |
| 13/10 04:12 | t7-ateliers | free | t7-avec2 | 3/12 | 31 | 340 s | même migration à la main, même plantage, avec l'aide ; la relecture (laguna) : rien à corriger |
| 13/10 04:23 | t7-ateliers | free | t7-avec3 | 12/12 | 56 | 353 s | avec la garde des fichiers JSON de données (T1-33) : la migration est signalée, le code lit l'ancienne forme. Cher : une dizaine de requêtes perdues à faire passer la forme décomposée « Zoe\u0301 » par le shell (la ligne des doublons dit maintenant de la tester dans un test unitaire) |
| 13/10 07:52 | t7-ateliers | free | t7-avec5 | 12/12 | 37 | 368 s | Claw au commit 48d68e6 (T1-32 complété, T1-33) : tout juste ; la relecture (laguna) fait deux remarques, prises en compte |
| 13/10 07:58 | t7-ateliers | free | t7-avec6 | 12/12 | 39 | 841 s | tout juste ; la garde des données JSON se déclenche (migration de `donnees.json`), le modèle rend le code tolérant ; aucun relecteur n'aboutit dans le temps (laguna, dots), d'où la durée |
| 13/10 04:34 | t7-ateliers | free | t7-avec4 | 11/12 | 27 | 297 s | confirmation : la normalisation des noms manque encore. Le modèle lit la ligne des doublons, puis : « the existing code doesn't do this for atelier names either », et suit l'ancien code (T1-32, suite). Rejoué au contrôle (message 39) avec la consigne complétée : 12/12 en 20 requêtes |
| 13/10 03:08 | t7-ateliers | free | t7-partie | 12/12 | 29 | 384 s | avec T1-32 (une ligne qui s'applique fait partie de la demande) : les noms sont comparés après `strip()`, `casefold()` et NFC |

Rejeu du contrôle de t7-avec (message 46) : 13/10 03:05, avec T1-32, 12/12 en 16 requêtes ;
le modèle écrit « This is a requirement from the engineering guide that applies here » et
normalise les noms. Mesures non indépendantes pour la phrase ajoutée (écrite après avoir vu
l'échec), mais le témoin `CLAW_EDGE_CASES=0` montre ce que fait le modèle sans aucune ligne.

Les essais de t5 ont été notés une seconde fois après une correction de la vérification
cachée : tous les points de la recherche dépendaient du premier (`?q=pomme` exact), si bien
qu'une seule erreur en coûtait huit (t5-north2 avait d'abord 4/12). La condition commune est
maintenant seulement que `q` filtre ; les autres notes ne changent pas.

Rejeux du contrôle des exigences de t5 (message 37) :

| Date | Formulation | Modèle | Résultat |
|---|---|---|---|
| 12/10 14:36 | celle de 806bf81 | free | 9/12, 2 requêtes : il confirme son travail |
| 12/10 14:37 | avec les cas limites d'une recherche (T1-24) | free | 12/12, 22 requêtes : décodeur standard (`parse_qs`), espaces retirés, accents et majuscules ignorés, plusieurs mots, tests ajoutés ; il lance aussi le serveur pour essayer l'API (aide des commandes non essayées) et l'arrête |
| 12/10 15:10 | relecture de t5b avec le rappel final de la forme (T1-23, suite) | dots (imposé) | écrit les deux étapes puis approuve, quand il avait répondu « VERDICT LGTM » nu pendant l'essai |
| 12/10 18:47 | contrôle des exigences, cas limites sous forme de pièges précis (T1-27) | free | 12/12 en 14 requêtes, contre 22 avec la forme générale |
| 12/10 22:58 | table revue, résultat attendu des essais (T1-28), banc de rejeu corrigé | free | 12/12 en 21 requêtes : la ligne de recherche réécrite ne fait rien perdre |

La session de cet essai a pu être lue pendant qu'il tournait : elle s'écrit maintenant à
chaque étape (T1-16).

Comparaison avec la série propre : scores 34/36 contre 32/36 ; 74 requêtes contre 99 (t2 :
10 contre 20 pour le plus puissant, 19 contre 36 pour le petit modèle). Le premier essai
t1 a d'abord été noté 5/6 par une vérification trop stricte, corrigée depuis (e3740fb).

Limites de cette mesure : un seul essai par configuration, avec des modèles qui ne répondent
pas deux fois pareil ; les écarts d'une ou deux requêtes ne prouvent rien. Sont nets et
expliqués par les traces : la fin des relectures après modification, le regroupement des
essais, et sur t3 l'hypothèse de l'utilisateur vérifiée puis démentie. La garde des données
ne s'est déclenchée dans aucun essai (le petit modèle n'a pas tenté de réécrire le CSV : le
guide complété a suffi) : elle n'est donc prouvée que par ses tests unitaires. Le conseil
sur le Python du Microsoft Store non plus (aucun détour par `/tmp`).

## Constats

### T1-01 La vérification des preuves coûte une requête quand la session montre déjà l'avant et l'après
- Preuve : t1 base (messages 7 à 22) et t3 base (messages 4 à 12) : le modèle a lancé la commande
  en échec, corrigé, relancé la même commande avec succès ; la remarque « Before this answer
  goes to the user: they reported a problem... » part quand même, et le modèle rend la même
  réponse.
- Remède : ne pas l'envoyer quand une même commande a échoué puis réussi après la dernière
  modification (cause observée, disparition observée).
- État : laissé tel quel : la demande de preuves coûte une requête mais rend la réponse du
  petit modèle mieux étayée ; l'utilisateur préfère la profondeur à la vitesse.

### T1-02 Après le contrôle des exigences, la réponse finale devient un tableau d'audit
- Preuve : t2 base, message 29 : « Vérification de chaque exigence » suivi d'un tableau ;
  le compte rendu destiné à l'utilisateur (message 22 : ce qui a été ajouté, comment
  l'utiliser) est remplacé, et c'est le dernier message que l'utilisateur lit.
- Remède : la remarque du contrôle demande de finir par le compte rendu pour l'utilisateur
  (résultat, ce qui a été fait, vérification), la vérification des exigences n'y entrant
  qu'en résumé.
- État : mineur, laissé : la consigne demande déjà de finir par le compte rendu.

### T1-03 L'hypothèse de l'utilisateur n'est ni vérifiée ni démentie dans la réponse
- Preuve : t3 base : « Je pense que le fichier ventes.csv est corrompu, tu peux le réparer ? ».
  Le modèle a bien corrigé le code, mais sa réponse ne dit jamais que le fichier n'est pas
  corrompu ni qu'il n'a pas été modifié (R1, R7).
- Remède : le guide de débogage dit qu'une cause avancée par l'utilisateur est une hypothèse
  à confirmer ou réfuter avec une preuve, et que la réponse le lui dit.
- État : corrigé (lot T1) : `DEBUGGER_STEPS` dit qu'une cause nommée par l'utilisateur est une
  hypothèse à vérifier, que des données valides ne se changent pas pour convenir au code, et
  la réponse dit si la cause soupçonnée était la bonne ; `PROBLEM_EVIDENCE_PROMPT` le redemande.

### T1-04 Le correctif n'est essayé que sur les données de l'exemple
- Preuve : t3 base : `replace(" ", "")` ; les espaces insécables des exports Excel en
  français (U+00A0, U+202F) font encore échouer la lecture ; aucun essai sur d'autres valeurs.
  La réponse conclut « Rien d'autre à faire » (R5, R6).
- Remède : à préciser après les essais avec des modèles plus petits.
- État : corrigé (lot T1) pour la consigne : l'étape 4 du guide demande un essai du correctif sur
  une entrée que l'exemple ne contient pas, par une commande ou un test. Effet à mesurer.
- Mesure (12 octobre, reprise de 16 h 22, voir T1-26) : sans effet sur t3 tant que le rappel
  restait général (« thousands separators (space or non-breaking space) ») ; 4/4 dès qu'il
  nomme le piège précis : `replace(" ", "")` ne retire pas U+00A0 ni U+202F, essayer
  « 1\u00a0250,00 ». Corrigé.

### T1-05 Le prompt système porte l'état et le diff Git de tout le dépôt, sans limite de taille
- Preuve : `prompt.rs`, `read_git_status` et `read_git_diff` (`git status --short --branch`,
  `git diff --cached`, `git diff`) sont lancés depuis le dossier de travail sans restriction de
  chemin, et `render_project_context` les recopie en entier. Essai t1 north : le projet était
  un sous-dossier du dépôt de Claw ; le diff (des centaines de lignes sans rapport) contenait
  la correction du scénario, et le modèle est allé lire le fichier de suivi qu'il y voyait.
- Défaut : dans un dépôt à plusieurs projets, ou un dossier de travail dans un grand dépôt, le
  modèle reçoit à chaque requête des modifications qui ne le concernent pas ; un gros diff
  (fichiers générés, `package-lock.json`) remplit le contexte d'un petit modèle et coûte des
  jetons à chaque étape.
- Remède : état et diff limités au dossier de travail (`-- .`), diff plafonné (au-delà, la
  liste des fichiers modifiés avec `git diff --stat` et une ligne qui dit de lancer
  `git diff` pour le détail).
- État : corrigé (lot T1) : `read_git_status` et `read_git_diff` sont limités au dossier de travail
  (`-- .`, `--relative`), l'état à 60 lignes, le diff à 6 000 caractères (au-delà : la liste
  des fichiers par `--stat`) ; une sortie Git qui n'est pas en UTF-8 ne fait plus tout perdre.

### T1-06 Le prompt système recopie les règles de permission de l'utilisateur
- Preuve : `claw system-prompt` : la section « Runtime config » recopie le JSON fusionné des
  réglages, dont la liste `permissions.allow` (ici 1,5 Ko de commandes `perl` et `PowerShell`
  venues de `~/.claude/settings.json`). Le modèle n'en fait rien : c'est Claw qui applique
  ces règles. La description d'une compétence externe y porte aussi un tiret long, que le
  même prompt interdit d'écrire.
- Remède : ne pas rendre la clé `permissions` (ni les secrets éventuels) dans le prompt ;
  passer les descriptions de compétences par le filtre typographique.
- État : corrigé (lot T1) : seuls `model`, `language`, `verify`, `review` et `permissionMode` sont
  montrés au modèle (la relecture a vu que `mcpServers`, `hooks` et les en-têtes des
  fournisseurs, donc des jetons, partaient aussi dans le prompt) ; les descriptions de
  compétences passent par le filtre typographique.

### T1-07 Le Python du Microsoft Store ne voit pas `/tmp` de Git Bash
- Preuve : t2 north, messages 34 à 46 : `mkdir /tmp/x && cp notes.py /tmp/x && cd /tmp/x &&
  python notes_test.py` échoue (`can't open file 'C:\\Users\\HP\\AppData\\Local\\Temp\\...'`).
  Le Python installé par le Microsoft Store a sa propre vue de `AppData` : il ne voit pas ce
  que Git Bash y écrit (et ce qu'il y écrit n'est pas vu des autres). Six requêtes perdues.
- Remède : quand une commande échoue ainsi (chemin sous `AppData\\Local\\Temp` introuvable
  pour un `python` de `WindowsApps`), le dire au modèle : travailler dans le dossier du
  projet, pas dans `/tmp`.
- État : corrigé (lot T1) : `store_python_hint`, joint au résultat de la commande en échec (avec
  l'aide aux chemins).

### T1-08 Un petit modèle « répare » les données de l'utilisateur au lieu du code qui les lit
- Preuve : t3 north, message 4 : sans avoir reproduit l'erreur, `write_file ventes.csv` avec
  le fichier entier réécrit de mémoire (« 12,5 » devient « 12.5 », « 1 250,00 » devient
  « 1250.00 », et « 80 » devient « 80.00 » sans raison). Le script n'est pas touché : le
  prochain export plantera de nouveau. Après la demande de preuves, il restaure le fichier,
  reproduit l'erreur, puis refait la même réécriture (messages 10 à 16).
- Défaut : l'hypothèse de l'utilisateur est prise pour une consigne (R1) ; des données valides
  dans leur format sont modifiées (R4) ; sur un vrai fichier, une réécriture de mémoire peut
  perdre des lignes.
- Remède : (1) une garde, sur le modèle de celle des tests réécrits : quand des lignes
  existantes d'un fichier de données sont remplacées, la remarque dit qu'un programme qui ne
  sait pas lire des données ne prouve pas qu'elles sont fausses, de restaurer le fichier et de
  corriger le lecteur si les valeurs sont valides dans leur format, sinon de dire exactement
  quelles lignes ont changé et pourquoi ; (2) le guide de débogage le dit avant (T1-03).
- État : corrigé (lot T1) : aide `CLAW_DATA_GUARD` (`is_data_file`, `replaced_lines`,
  `data_guard_feedback`, `rewritten_data`). La remarque cite les lignes d'origine pour une
  remise en place exacte, ne se déclenche pas sur un fichier créé pendant le tour, et se
  réarme quand le fichier est restauré. Reste T1-11.

### T1-09 Le modèle relit un fichier après chacune de ses propres modifications
- Preuve : t2 north : 13 lectures de `notes.py` (60 lignes) pour 5 modifications, dont trois
  tranches de 30 lignes d'affilée ; t1 north : 5 relectures ; t2 clean (nemotron) : 3. Chaque
  relecture coûte une requête et remet le fichier dans le contexte.
- Cause : le résultat allégé d'une modification ne montre plus rien du fichier ; le modèle ne
  sait plus quel est le texte exact à citer pour sa modification suivante.
- Remède : le résultat d'une modification réussie porte le fichier tel qu'il est maintenant
  s'il est petit, sinon les lignes autour du changement, avec « inutile de le relire ».
- État : corrigé (lot T1) : aide `CLAW_FILE_VIEW` (`view_after_edit`). Petit fichier montré en
  entier, grand fichier montré autour du changement (trouvé en comparant l'avant et l'après,
  lignes et taille bornées), une ligne après `write_file`. Une seule vue à jour par fichier
  (`replace_file_views`) ; une vue est retirée dès que le fichier change par un autre moyen
  (`drop_stale_file_views`) et à la réouverture d'une session. Reste T1-12.

### T1-10 Des vérifications indépendantes lancées une par requête
- Preuve : t2 clean, messages 20 à 28 et 34 à 38 : sept commandes `python notes.py chercher
  ...` chacune dans sa requête ; t2 north, messages 50 à 60 : six ; t1 north, messages 24 à
  31 : quatre `python -c "print(...)"`. Le prompt système demande déjà de regrouper ; les
  modèles le font au début du tour, plus ensuite.
- Remède : après trois réponses de suite faites d'un seul appel en lecture ou d'une seule
  commande réussie, une remarque jointe au résultat : regrouper les essais indépendants dans
  une commande ou dans une même réponse.
- État : corrigé (lot T1) : aide `CLAW_BATCH_CALLS`, `BATCH_NOTE` jointe au troisième appel isolé
  de suite, une fois par tour. Effet à mesurer.

### T1-11 Un fichier de données réécrit par une commande n'est pas vu
- Preuve : relecture du lot T1. `sed -i`, un script Python ou `cat > ventes.csv` changent le
  fichier sans passer par `edit_file` ni `write_file` ; l'instantané du projet
  (`project_snapshot`) ne suit que les extensions de code.
- Remède : suivre aussi les fichiers de données dans l'instantané, sans déclencher pour eux
  la vérification du code.
- État : corrigé (reprise du 12 octobre, 16 h 22, suite) : `DataSnapshot` note au début du tour
  les fichiers de données du dépôt (suivis ou nouveaux, 200 au plus) avec leur date et leur
  taille, et le contenu de départ de ceux qui ne sont pas dans leur état du dernier commit (Git
  garde les autres). Après chaque commande du modèle, un fichier changé est comparé à son
  état de départ : des lignes remplacées ou retirées déclenchent la garde des données, une
  fois par fichier, avec la commande en cause (`command_data_guard_feedback`). Un fichier
  seulement complété ne compte pas ; les modifications du modèle par `edit_file` et ce
  qu'écrit la vérification de Claw ne sont pas attribués à une commande. Vérifié de bout en
  bout avec le faux modèle (`printf ... > ventes.csv` dans un dépôt Git : notice et remarque
  avec la ligne d'origine).

### T1-12 Après `/undo`, la page réaffiche la vue du fichier en entier
- Preuve : relecture du lot T1. Dans le fil rejoué, la vue jointe au résultat de la dernière
  modification de chaque fichier apparaît sous « Remarque de Claw au modèle » (jusqu'à 4 000
  caractères). À la réouverture d'une session ce n'est plus le cas (vues remplacées).
- Remède : replier la vue en une ligne dans la page.
- État : corrigé (reprise du 12 octobre, 6 h 22) : dans la page, `clawNote` replie la vue en une
  ligne dépliable.

### T1-13 Après une relecture sans reproche, la réponse finale n'est plus qu'un accusé de réception
- Preuve : t1 lotT1 (free), session `20261012-022030-t1-inventaire-lotT1`. Message 18 : compte
  rendu complet (1 467 caractères : résultat, causes, corrections, vérification). Message 19 :
  relecture par un autre modèle, aucun point à corriger. Message 20, le seul que l'utilisateur
  lit : 270 caractères, « La revue confirme que les deux corrections sont correctes et
  minimales... ». La page replie le message 18 comme « réponse provisoire ».
- Défaut : chaque contrôle de fin de tour qui ne trouve rien à redire peut remplacer un bon
  compte rendu par un commentaire du contrôle (T1-02 en est la forme atténuée). La consigne
  finale (`FINAL_ANSWER_RULE`) le demande pourtant ; un modèle qui n'a rien à changer répond
  au contrôle au lieu de réécrire son compte rendu.
- Remède, sans requête de plus : quand la réponse à un contrôle n'appelle aucun outil et est
  nettement plus courte que la réponse qu'elle remplace (par exemple moins de la moitié), la
  réponse précédente reste la réponse finale : rien n'a changé entre les deux, et les deux
  textes sont du modèle. À faire dans la session (le dernier message porte le compte rendu) et
  dans l'interface (la réponse repliée est rouverte, au lieu d'être marquée provisoire).
- État : corrigé (reprise du 12 octobre, 6 h 22) : aide `CLAW_KEPT_ANSWER`. Quand la réponse à un
  contrôle n'appelle aucun outil et fait moins du tiers d'un compte rendu d'au moins 400
  caractères, `keep_draft_answer` remet ce compte rendu devant elle dans le dernier message
  (session et résultat du tour), et `answer_kept` fait rouvrir à la page la réponse repliée.
  À confirmer sur un essai réel quand le cas se représente.

### T1-14 Chaque mise à jour de la liste de tâches coûte une requête
- Preuve : t4 (free), réponses 2, 4, 6, 8, 10, 12 et 28 : `TodoWrite` seul dans la réponse,
  entre deux modifications. 7 requêtes sur 31.
- Remède : quand une réponse ne fait que mettre la liste à jour, le résultat de `TodoWrite`
  dit de l'envoyer avec l'appel suivant, dans la même réponse.
- État : corrigé (reprise du 12 octobre, 6 h 22, suite) : `lone_call_note` joint `TODO_ALONE_NOTE`
  au résultat d'un `TodoWrite` envoyé seul quand la liste garde des tâches à faire
  (`todos_left`), trois fois au plus par tour (aide `CLAW_BATCH_CALLS`) ; la consigne du
  prompt et la description de l'outil le demandent d'avance. Le résultat gardé dans la session
  ne répète plus l'ancienne et la nouvelle liste (`lean_todos` : nombre de tâches, faites, en
  cours) : sept mises à jour répétaient 15 000 caractères à chaque requête suivante.
- Mesure (t4c, le plus puissant) : une liste envoyée seule au début (remarque reçue), puis les
  cinq mises à jour suivantes jointes à une modification ou à une commande ; la dernière,
  seule, ferme la liste (pas de remarque). Deux réponses de liste seule contre sept.

### T1-15 La remarque de regroupement arrive tard et une seule fois
- Preuve : t4 (free), réponses 15 à 26 : douze commandes `python biblio.py ...` une par
  requête. La remarque n'est arrivée qu'après la septième, parce qu'une commande au code de
  sortie non nul (ici des refus du guichet, essayés exprès) remettait le compteur à zéro ;
  après elle, cinq commandes isolées de plus.
- Remède : compter aussi les commandes au code de sortie non nul ; redire la remarque quand
  les appels isolés continuent (plus précise : enchaîner les commandes avec `;` dans un seul
  appel), trois fois au plus par tour.
- État : corrigé (même reprise) : `lone_call_note` compte toute observation isolée (seul un
  outil en erreur ou une réponse d'un autre genre remet le compteur à zéro) ; `batch_note` est
  redite à la 6e et à la 9e réponse isolée (`MAX_BATCH_NOTES`), plus ferme, et montre les trois
  dernières commandes du modèle réunies en un appel (`chained_commands` : `cd dossier &&`
  commun écrit une fois, rien si une commande ne s'enchaîne pas).
- Mesure (t4b-north) : les trois remarques sont arrivées aux réponses isolées 3, 6 et 9, avec
  l'exemple tiré de ses commandes ; le petit modèle a continué une commande par réponse (onze
  d'affilée). Sur lui, la remarque ne change rien.
- Mesure (t4c, le plus puissant) : après la remarque de la 6e réponse isolée, il enchaîne
  deux commandes par appel (`rendre L1 && etat`), après celle de la 9e, quatre ; mais il garde
  un appel par réponse : 14 commandes en 14 requêtes, comme au premier essai. Effet partiel.

### T1-16 Un tour interrompu ne laisse aucune trace : la session n'est enregistrée qu'à la fin
- Preuve : t4 (north), arrêté après 25 minutes : le fichier de session fait 27 octets.
  `persist_session` (claw-cli) n'est appelé qu'après le tour ; rien n'est écrit pendant.
- Défaut : fermer le terminal ou VS Code, un plantage ou une coupure pendant un long tour
  perd toute la conversation du tour (on ne peut ni la relire ni la reprendre), alors que les
  fichiers, eux, ont été modifiés.
- Remède : enregistrer la session après chaque étape (écriture dans un fichier temporaire
  puis renommage, pour ne jamais laisser un fichier à moitié écrit).
- État : corrigé (même reprise) : `ConversationRuntime::autosave_to`, appelé avant chaque tour
  par le terminal (trois modes), la page et ACP. La session est écrite avant chaque requête au
  modèle, dès qu'une réponse demande des outils (les appels en cours y portent un résultat
  « Claw was stopped while this call was running », pour que la session reste valide si Claw
  s'arrête pendant leur exécution) et à la fin du tour, réussi ou non. `Session::save_to_path`
  écrit à côté puis renomme. Vérifié de bout en bout sur `claw.exe` avec le faux modèle (arrêt
  forcé pendant un `sleep 40` : trois messages enregistrés, aucun fichier temporaire laissé).

### T1-17 Le petit modèle relit chaque fichier juste avant de le modifier
- Preuve : t4b-north, messages 12, 16, 20 et 24 : `read_file` de `services.py`, `stockage.py`,
  `biblio.py` et `test_services.py`, tous lus au message 1 et inchangés depuis (« Tout
  d'abord, la vue du fichier »). Quatre requêtes sur les dix premières, et chaque fichier deux
  fois dans le contexte.
- Remède : quand une lecture rend exactement ce qu'une lecture encore présente dans la
  conversation a rendu (même fichier, mêmes lignes, même contenu), ne garder que la plus
  récente ; si les deux sont du même tour, le dire au modèle.
- État : corrigé (reprise du 12 octobre, 6 h 22, suite) : aide `CLAW_REPEATED_READS`
  (`repeated_read_note` dans `conversation.rs`, `read_result`, `repeated_read_stub`). Deux
  remarques au plus par tour ; une relecture d'un tour à l'autre n'est pas reprochée (le
  fichier avait pu changer), mais une seule copie reste.
- Mesure (t5-north2) : 6 lectures identiques retirées du contexte, 2 remarques ; le petit
  modèle relit encore ses fichiers après ses propres modifications (16 lectures). Effet limité.

### T1-18 Une exigence mal lue passe le contrôle des exigences et la relecture
- Preuve : t4b-north, session `20261012-080214-t4-bibliotheque-t4b-north`. La demande dit
  « Quand le livre est rendu, il est mis de côté pour le premier de la file : lui seul peut
  alors l'emprunter ». Le modèle fait prêter le livre d'office au premier de la file (message
  38 : « la logique s'attend à ce que après rendre L1, le livre soit attribué à Bilal »),
  l'essaie ainsi (messages 36 à 45), et son contrôle des exigences (message 69) le confirme
  avec la même lecture. La relecture par un autre modèle n'a rien renvoyé : elle n'a
  vraisemblablement pas abouti (T1-20). Même erreur dans t4c, avec le plus puissant.
  Quatre points perdus sur douze, dont un pour une exigence implicite (réserver le livre qu'on
  a déjà ; réserver un livre disponible est accepté aussi).
- Défaut : celui qui a mal lu une phrase se relit avec la même lecture ; le relecteur reçoit
  le diff et la réponse de l'agent, qui présentent cette lecture comme acquise, et rien ne
  l'oblige à repartir des phrases de la demande. Quand il approuve, sa réponse n'est gardée
  nulle part : on ne peut pas savoir ce qu'il a vu.
- Remède : (1) la relecture demande au relecteur, avant de lire le diff, de traduire chaque
  phrase de la demande en un comportement observable (une commande ou un appel, et son
  résultat attendu), puis de dire où le code fait autre chose, même si c'est une variante
  raisonnable ; (2) le contrôle des exigences demande la même chose à l'agent : citer la
  phrase, dire ce que l'utilisateur observerait au mot près, l'essayer ; (3) garder la réponse
  du relecteur dans un journal.
- État : en partie corrigé (reprise du 12 octobre, 11 h 22) : (1) `REVIEW_SYSTEM` fait d'abord
  traduire chaque phrase de la demande en comportement observable, puis suivre le code pour
  chacun ; la réponse de l'agent vient après le diff, comme des affirmations à vérifier ; seul
  le verdict (après `VERDICT`) repart à l'agent, et une approbation en fin de réponse est
  reconnue ; (2) le contrôle des exigences fait relire chaque phrase telle qu'elle est écrite,
  dire ce que l'utilisateur doit observer, et ne pas faire automatiquement une action que la
  demande laisse à une personne (« the manager can then approve it ») ; (3) `CLAW_REVIEW_LOG`
  garde chaque réponse du relecteur (le banc les met dans `result.json`), et `claw
  review-replay` avec `review_replay.py` rejoue une relecture sur un essai enregistré.
- Mesures : rejouée sur t4c avec ses moyens corrigés (T1-20), la relecture aboutit (laguna,
  8 s) mais sans les étapes demandées : « VERDICT / LGTM », le prêt d'office approuvé. Une
  lecture indépendante de la demande par un autre modèle, sans le code, a été essayée sous
  deux formes (deux requêtes, dots) : en critères, elle garde la bonne lecture (« set aside for
  the first member in the reservation queue, and only that member may borrow it next ») mais
  reste abstraite ; en scénarios, elle reprend l'erreur (« rendre L ; the book is assigned to
  M2 ») puis se contredit (« emprunter M2 L ; the borrow succeeds »). Trois modèles sur quatre
  lisent cette phrase ainsi : un second lecteur ne corrige pas sûrement une lecture par défaut
  partagée, la consultation n'est pas retenue. L'effet de (2) reste à mesurer sur un essai réel.
- Mesure de (2) par rejeu du contrôle sur t4c (tableau des rejeux) : sans effet. Le modèle
  traduit correctement la phrase puis affirme que son prêt d'office la respecte ; la règle sur
  l'action laissée à une personne ne lui fait pas voir la contradiction avec sa propre sortie.
  Gardée (générale, sans coût), mais ce n'est pas elle qui protège de ce défaut. Ce qui a
  trouvé de vrais défauts : la relecture par un autre modèle quand il fait les deux étapes
  (dots, deux fois sur deux) ; d'où T1-23.
- Suite (13 octobre, 9 h 30) : t4 refait à 10/12, le prêt d'office revient et aucun relecteur
  ne répond à temps (T1-35). Rejouée sur ce travail, la relecture par le modèle de la session
  (nemotron) lit elle aussi « auto-checkout to next in line » et approuve. Claw extrait
  maintenant les phrases de la demande qui laissent une action à une personne (« peut
  alors », « seul peut », « can then »...) et les cite telles quelles au contrôle et au
  relecteur. Même travail, même relecteur, phrase citée : « they must actively run
  `emprunter`, it is not auto-checked-out », et le défaut de `rendre` est signalé. Au
  contrôle, en revanche, l'agent écrit encore « lui seul peut alors l'emprunter - Done » :
  celui qui a écrit le code le défend ; c'est la relecture (autre modèle, ou le même en
  secours, T1-35) qui rattrape. t4 refait ensuite : 12/12 (la relecture, dots, lit juste).

### T1-19 Rien ne borne le nombre de requêtes d'un tour
- Preuve : t4-north (07:06) : 250 requêtes en 25 minutes, plus aucun fichier modifié après
  la 6e minute ; seul le délai du banc l'a arrêté. Pour l'agent principal, `max_iterations`
  vaut `usize::MAX` ; le garde-fou (`StallGuard`) ne fait que des rappels (même appel et même
  résultat trois fois ; 40 appels d'outils, une fois), et des commandes d'essai qui changent
  l'état à chaque fois ne se répètent jamais à l'identique. Les relances du client de l'API,
  elles, sont bornées (`BROKEN_STREAM_RETRIES`, `UPSTREAM_SWITCHES`) : ce sont bien des
  réponses du modèle. Le second essai (t4b-north) s'est terminé en 34 requêtes : le cas n'est
  pas reproduit, et le premier n'a pas laissé de trace (T1-16, corrigé depuis).
- Défaut : un modèle qui tourne en rond peut dépenser en un seul tour le quota gratuit de la
  journée (1 000 requêtes), sans que personne le voie.
- Remède : un budget de requêtes par tour (réglage, 80 par défaut ; les tours réels de
  l'utilisateur vont jusqu'à 35). Arrivé là, Claw demande au modèle de conclure (ce qui est
  fait et vérifié, ce qui reste), n'exécute plus d'outil et rend la main ; l'utilisateur dit
  « continue » pour poursuivre.
- État : corrigé (reprise du 12 octobre, 11 h 22) : 100 requêtes par tour (les tours de t4
  en font 30 à 35), réglage `turnRequestLimit`, variable `CLAW_TURN_REQUESTS`, 0 pour ne pas
  borner. Au-delà : notice à l'utilisateur, note au modèle (`turn_limit_note`), qui conclut
  sans outil ; un appel demandé quand même ne tourne pas et reçoit son résultat
  (`turn_limit_refusal`) ; ni contrôle ni relecture ne relancent alors le travail, un hook
  `Stop` non plus. Vérifié de bout en bout sur `claw.exe` avec le faux modèle (limite à 1 :
  deux requêtes, la notice dans la sortie JSON).

### T1-20 La relecture par un autre modèle n'aboutit pas sur un vrai diff
- Preuve : relecture rejouée sur le travail de t4c avec les consignes et les moyens de ce
  matin (`review_replay.py --baseline 93d40ae`) : `laguna` « réponse inachevée après 50 s »,
  `dots` « réponse sans texte », `ling` « réponse inachevée après 2 s » (plus de temps). Une
  consultation avait 3 000 jetons de sortie, 50 s par modèle et 90 s en tout, et Claw demande
  aux modèles qui raisonnent leur plus haut niveau d'effort. Avec 8 000 jetons et le niveau
  « medium » : `laguna` 27 664 caractères de raisonnement, `dots` 33 457, arrêtés sur la
  limite sans un mot de réponse. Ces modèles n'ont pas de niveaux d'effort dans le catalogue :
  seule la place de sortie les borne. Sur t1 (petit diff), la relecture aboutissait (T1-13).
- Défaut : sur une tâche de plusieurs fichiers, le travail partait sans relecture, sans que
  rien le dise dans les modes sans affichage (T1-21), pour une à trois requêtes perdues.
- État : corrigé (même reprise) : la relecture a 20 000 jetons de sortie
  (`REVIEW_MAX_TOKENS`), 240 s par relecteur et 360 s en tout (`Patience::REVIEW`) ; les
  consultations plafonnent le raisonnement à « medium » (`cap_thread_reasoning`, propre au fil
  de la consultation) ; un relecteur dont la réponse est partie tout entière en raisonnement
  est consulté une seconde fois quand son niveau a pu baisser ; un échec dit ce qui est arrivé
  (`empty_reply_reason` : « réponse sans texte après 27664 caractères de raisonnement (arrêt :
  length) »). Rejouée ainsi, la relecture de t4c a abouti (laguna, 8 s cette fois).

### T1-21 Sans affichage, les notices de Claw sont perdues
- Preuve : `output.json` des essais de t4 (`--output-format json`) : aucune notice, ni de la
  vérification, ni de la relecture (faite, approuvée ou en échec) ; `stderr.txt` vide. On ne
  pouvait pas savoir si la relecture avait eu lieu.
- État : corrigé (même reprise) : la sortie JSON porte `notices`, tout ce que Claw a signalé
  pendant le tour (`start_collecting_notices`, `take_notices`) ; le banc les garde dans
  `result.json` avec les réponses du relecteur (`reviews`).

### T1-22 La vérification automatique ne trouve pas des tests écrits pour pytest
- Preuve : t4d, notices de Claw : « Vérification automatique après modification : python -m
  unittest discover -s tests -q », puis « Vérification sans objet : aucun test n'a été trouvé ;
  le travail n'est pas vérifié », pour un projet dont douze tests pytest passaient.
  `python_verify` ne choisit pytest que s'il est réglé (`pytest.ini`, `conftest.py`, section
  de `pyproject.toml`...) ; des tests qui importent pytest ou ne sont que des fonctions
  `test_...`, sans réglage, partaient à `unittest`, qui ne les voit pas.
- Défaut : sur un projet Python courant, la vérification de Claw ne vérifiait rien et le
  disait au modèle.
- État : corrigé (même reprise) : `tests_written_for_pytest` lit les fichiers de test (racine,
  `tests/`, `test/` et leurs sous-dossiers, 40 au plus) ; s'ils importent pytest ou n'ont que
  des fonctions `test_...`, et que `python -m pytest --version` répond (`pytest_installed`,
  une fois par processus), la vérification est `python -m pytest -q -x`.

### T1-23 Une approbation sans analyse passe pour une relecture
- Preuve : rejeux sur t4c (tableau). `laguna` répond « VERDICT / LGTM » en 8 s sans les deux
  étapes demandées et approuve un travail fautif ; `dots`, qui les fait, y trouve un vrai
  bogue. Dans t4d, `dots` trouve aussi un vrai défaut. Et `dots` termine sa liste de
  problèmes par « LGTM » : la règle qui lisait un « LGTM » en dernière ligne comme une
  approbation aurait jeté ses remarques.
- État : corrigé (même reprise) : `bare_approval` (une approbation sans « Step 1 » ni
  « Step 2 ») fait consulter un second relecteur, autre que le premier (`consult_again`), dont
  l'avis remplace le premier ; dans le verdict, un « LGTM » n'efface plus les problèmes qui
  citent un emplacement du code (`cites_a_location`) ; « NOT LGTM » est un refus. Une
  consultation vide dit si le modèle a été redemandé.
- Mesure (t5b) : `dots` répond « VERDICT LGTM » nu, le second relecteur (nemotron-super)
  aussi. Suite : la demande au relecteur se termine par un rappel de la forme exigée, même
  quand tout est juste (`REVIEW_FORMAT_REMINDER`) ; rejouée sur t5b, la relecture de `dots`
  écrit alors les deux étapes. Deux approbations nues sont dites à l'utilisateur (« le travail
  n'a pas vraiment été relu »).

### T1-24 Les cas limites qu'un ingénieur vérifie d'office sont oubliés
- Preuve : t5 (free), 9/12 : la recherche ne retire pas les accents (« creme » ne trouve pas
  « Crème brûlée »), lit la requête à la main avec `unquote` (un `+` n'est pas un espace) et
  ne retire pas les espaces (« %20%20 » ne trouve rien). Le contrôle des exigences parle des
  cas limites en général (« empty or missing input, boundaries... ») ; le modèle a vérifié la
  recherche vide, pas le reste. La relecture (laguna, avec analyse) ne les a pas vus non plus.
- Défaut : un ingénieur expérimenté a, pour chaque genre de fonctionnalité, une liste de
  vérifications qu'il fait sans qu'on les lui demande ; un modèle gratuit ne s'en souvient pas.
- État : corrigé (reprise du 12 octobre, 11 h 22, suite) : `FEATURE_EDGE_CASES`, neuf genres
  (recherche ou filtre, montants, dates, CSV et Excel, pagination, formulaires, envois de
  fichiers, comptes et mots de passe, tris), reconnus à des débuts de mots de la demande
  (minuscules, sans accents, demande poursuivie comprise) ; au plus quatre sont ajoutés au
  contrôle des exigences, avec la consigne de montrer le test ou la commande qui couvre chacun,
  ou de l'ajouter. `claw checklist-note` imprime le contrôle exact pour le banc.
- Mesure : rejeu du contrôle de t5 au message 37 (tableau) : 9/12 avec l'ancienne formulation,
  12/12 avec les cas limites, pour 22 requêtes de plus. Confirmé sur un essai complet (t5b :
  12/12 contre 9/12) ; avec le petit modèle, 10/12 (accents, espaces et décodage justes). À
  essayer sur un autre genre de fonctionnalité.

### T1-25 Sans personne pour décider, une limite passagère du fournisseur arrête le tour
- Preuve : t5-laguna, `stderr.txt` : « l'api a échoué après 3 tentatives : 429 ... is
  temporarily rate-limited upstream. Please retry shortly ». En mode interactif, Claw réessaie
  seul après 3 s, 10 s, 30 s... (essais automatiques de la question de changement de modèle) ;
  sans personne pour répondre (`claw prompt`, script, action GitHub, banc) et sans modèle de
  secours, le tour s'arrêtait après les trois essais rapprochés du client de l'API.
- État : corrigé (reprise du 12 octobre, 11 h 22, suite) : dans ce cas, un échec passager
  (`failure_is_transient` : 429 hors quota du jour, 408, 409, 5xx) est réessayé après 10 s,
  30 s, 1 min, 2 min, 3 min puis 4 min (`unattended_retry_delay`, une dizaine de minutes ; quatre
  minutes d'abord, trop peu : voir t5-laguna2), avec une notice à
  chaque fois ; l'attente s'interrompt avec le tour (`model_switch::pause`). Le faux modèle a un
  cas « amont-limite » (429 aux quatre premières requêtes) : avant, le tour échouait en 4 s ;
  après, il aboutit en 16 s avec une notice.
- Suite (t5-laguna2) : une limite a duré plus de quatre minutes ; l'attente va maintenant à une
  dizaine de minutes (six essais, jusqu'à 4 min d'écart). Et un tour en échec en mode
  `--output-format json` n'imprimait rien : il rend maintenant `{"error", "model", "notices"}`,
  pour qu'un script sache ce qui est arrivé (vérifié avec le faux modèle « introuvable »).

### T1-26 Les cas limites n'arrivent qu'au contrôle des exigences, que les petites corrections n'atteignent pas
- Preuve : t3 (CSV « corrompu ») : la correction fait deux ou trois lignes, le contrôle des
  exigences (cinq lignes au moins) n'arrive jamais, et pour un problème signalé c'est la
  demande de preuves qui le remplace. Les cas limites du CSV (espaces insécables d'Excel)
  n'étaient donc jamais rappelés ; aucun essai de t3 ne les traitait (T1-04).
- État : corrigé (reprise du 12 octobre, 16 h 22) : aide `CLAW_EDGE_CASES`. Les cas limites du
  genre de fonctionnalité sont joints à la demande dès le début du tour (`edge_case_guide`,
  bloc de guide masqué à la relecture d'une session), et rappelés, chacun à prouver, au
  contrôle des exigences comme à la demande de preuves d'un problème (`edge_case_check`).
- Mesures sur t3 (modèle le plus puissant, une fois chacune) : guide au début seul, 3/4, le
  modèle ne s'en sert pas ; avec le rappel dans la demande de preuves, 3/4, il n'en dit pas un
  mot ; avec un rappel qui nomme le piège précis (ci-dessus), 4/4. Ce qui fait agir le modèle :
  le piège concret et l'essai à faire, pas la catégorie.

### T1-27 Un cas limite écrit en catégorie ne fait pas agir le modèle
- Preuve : T1-26 et T1-04 : sur t3, « thousands separators (space or non-breaking space) »,
  rappelé deux fois, n'est pas même mentionné par le modèle ; « `replace(" ", "")` ne retire
  pas U+00A0 ni U+202F, essayer « 1\u00a0250,00 » » fait 4/4.
- État : corrigé (reprise du 12 octobre, 16 h 22) : chaque ligne de `FEATURE_EDGE_CASES` nomme
  le piège précis et l'essai à faire (recherche : `normalize("NFD")`, `parse_qs` et pas
  `split("=")` ; montants : `0.1 * 3 == 0.3` est faux, compter en centimes ou `Decimal` ; dates :
  « 03/04/2026 » est le 3 avril ; pagination, formulaires, envois de fichiers, comptes, tris de
  même). Mesuré sur la recherche (rejeu de t5) : 12/12 en 14 requêtes au lieu de 22 ; les
  autres genres n'ont pas encore de scénario.

### T1-28 Des cas limites dont certains faits étaient faux, et qu'un modèle lit à sa façon
- Preuve : t6 (10/12) : une période à l'envers comptée, un solde à -3 ; la table ne parlait ni
  de soldes ni de période à l'envers. Revue de la table par deux agents (lecture seule, chaque
  fait rejoué sur cette machine) : 18 affirmations fausses ou incomplètes. Par exemple `==`
  entre une date avec fuseau et une sans ne lève rien, il répond faux ; `parse_qs("q=")` donne
  `{}`, donc `["q"]` plante sur la recherche vide que la ligne demandait ; NFD garde « œ », et
  « oeufs » ne trouvait pas « Œufs » ; Windows écrit U+202F dans « 1 250,00 », pas U+00A0 ;
  `cgi` et `imghdr` n'existent plus en Python 3.13. Des mots déclenchaient aussi le mauvais
  genre : « sort » dans « sortie » et « fais en sorte », « form » dans « format », « excel »
  dans « Excellent », « connexion » dans « erreur de connexion », « par page » dans « page par
  page », « heure » dans « tout à l'heure » (sur un corpus écrit pour l'occasion : 52 genres de
  trop pour 71 demandes ordinaires, 26 manqués pour 62 demandes de fonctionnalité).
- Puis, au rejeu de t6 avec la table corrigée : encore 10/12. Le modèle fait les essais qui
  confirment son code et garde l'échange automatique des dates (« 5 (échange auto) »), que la
  ligne interdisait pourtant.
- État : corrigé (reprise du 12 octobre, 21 h 22). La table a son module (`edge_cases.rs`) :
  formulations corrigées, faits rejoués ; mots-clés en expressions, et un mot suivi d'une
  espace est un mot entier, aussi devant une ponctuation (`has_word_start`) ; l'apostrophe des
  téléphones vaut la droite. Quatorze genres nouveaux, faits vérifiés une seconde fois de façon
  indépendante : soldes, stocks, quotas et réservations ; doublons ; adresses et téléphones ;
  bases SQL ; clés d'API ; OCR et traduction ; images ; appels à un service extérieur ; liaison
  front et back ; API JSON ; code de page web ; données en JSON ; fichiers texte sous Windows ;
  scripts en ligne de commande, en dernier car le plus large (quatre genres au plus, dans
  l'ordre de la table). Chaque essai porte son résultat attendu entre parenthèses, et le
  contrôle dit qu'un autre résultat veut dire un code faux, même voulu (sauf si la demande le
  veut). Le relecteur reçoit aussi ces lignes, à confronter au code.
- Mesures (non indépendantes pour t6) : rejeu du contrôle, 10/12 puis 12/12 (40 requêtes au
  lieu de 9 à 11) ; essai complet, 12/12 en 32 requêtes (le premier faisait 10/12 en 19). Le
  coût : le modèle fait maintenant ses essais un par un, une requête chacun.
- Reste : mesurer les genres nouveaux sur des scénarios à eux (seuls soldes et ligne de
  commande ont servi, sur t6) ; faire partir les essais d'une ligne en une seule commande.

### T1-29 Une commande du modèle efface les modifications qu'il venait de faire
- Preuve : t6 avec le petit modèle (essai arrêté à 81 requêtes). Après un `edit_file` refusé
  (le bloc qu'il croyait en double n'existait pas ; Claw lui montrait les lignes réelles), il
  passe par le shell : `sed -n '1,83p' conges.py > conges_fixed.py && ... && rm tail.txt
  conges.py conges_fixed.py` supprime `conges.py` ; il le restaure par `git checkout
  conges.py`, qui ramène la version d'origine : ses modifications du tour sont perdues et il
  recommence. Deux fois dans le même essai, une trentaine de requêtes en tout.
- Remède proposé : Claw garde la dernière version de chaque fichier que le modèle a modifié
  avec ses outils pendant le tour. Après une commande, si l'un d'eux a disparu ou est revenu à
  son état du début du tour, Claw le dit au modèle et lui donne de quoi le restaurer (une copie
  gardée sous `.claw/`), sans le faire à sa place : une suppression peut être voulue.
- État : corrigé (reprise du 12 octobre, 21 h 22, suite) : aide `CLAW_LOST_EDITS`. Avant la
  première modification d'un fichier par les outils du modèle, Claw relève son contenu ; après
  chaque modification, la version écrite. Après une commande (`bash`, `PowerShell`), un de ces
  fichiers supprimé, ou revenu au contenu du début du tour aux fins de ligne près (Git peut
  les changer en restaurant), est signalé au modèle avec la commande `cp` qui remet sa
  dernière version, gardée sous `.claw/kept/` (dans le dossier temporaire pour un fichier
  hors du projet), et à l'utilisateur. Un fichier changé autrement (formateur, `sed -i`)
  devient la version de référence ; un fichier créé puis effacé dans le tour ne compte pas.
  Testé avec un faux modèle et de vrais fichiers, puis de bout en bout avec le vrai binaire
  et ses vrais outils (le faux modèle sait maintenant enchaîner plusieurs appels dans un tour,
  `[mock:steps ...]`) : `rm` puis `git checkout` sont vus, la copie gardée est la dernière
  version du modèle. Pas encore vu dans un essai réel.

### T1-30 Des mots qui ne disent pas le genre : « inscription » à un atelier
- Preuve : la demande du scénario t7, écrite d'abord comme on la dirait (« Ajoute les
  inscriptions aux ateliers [...] tant qu'il reste des places [...] Une même personne ne doit
  pas être inscrite deux fois »), déclenchait les comptes et les mots de passe (le mot
  « inscription ») et la ligne de commande, et ni les places ni les doublons.
- État : corrigé (reprise du 12 octobre, 21 h 22, suite). « inscription » seule ne vaut plus
  un compte (« page d'inscription », « formulaire d'inscription », « inscription des
  utilisateurs » oui) ; places : « reste des places », « nombre de places », « places
  limitées », « liste d'attente »... ; doublons : « deux fois ». La demande de t7 donne
  maintenant soldes et places, doublons, ligne de commande. Mesure non indépendante pour la
  reconnaissance (mots choisis après avoir vu la demande) ; l'effet des lignes elles-mêmes se
  mesure sur t7 avec et sans l'aide (`CLAW_EDGE_CASES=0`).

### T1-31 Un script que sa commande ne lance plus : `main()` jamais appelée
- Preuve : t6 avec le petit modèle : dans une réécriture de `conges.py`, le bloc `if __name__
  == "__main__":` disparaît. `python conges.py jours ...` n'écrit plus rien (code 0) ; le
  modèle le remarque (« the command didn't produce any output ») puis appelle `main()` par
  `python -c "from conges import main; ..."`. Ses tests, qui appellent `main()` directement,
  passent ; la commande de l'utilisateur ne fait rien (2/12 en l'état).
- État : corrigé (reprise du 13 octobre, 2 h 22) : aide `CLAW_ENTRY_POINT`. Une commande qui
  lance un script `.py` du projet, réussit et n'écrit rien sur la sortie ni sur les erreurs :
  si le script définit `main()` au premier niveau sans l'appeler nulle part (ni `__name__`, ni
  appel de `main(`), Claw le dit au modèle, une fois par script et par tour, avec le bloc à
  remettre. Testé avec de vrais fichiers, puis de bout en bout avec le vrai binaire.

### T1-32 Le modèle lit le cas limite, puis l'écarte : « la demande ne le dit pas »
- Preuve : t7 avec l'aide (11/12, comme le témoin sans l'aide). La ligne des doublons arrive
  au début du tour et au contrôle ; le modèle l'applique à son code et conclut lui-même que
  « Zoé », « zoé » et la forme décomposée compteraient pour trois personnes, puis : « the
  request doesn't explicitly ask for it ». Les cas d'expert lui semblaient facultatifs.
- État : corrigé (reprise du 13 octobre, 2 h 22) : le contrôle dit qu'une ligne qui
  s'applique fait partie de ce que l'utilisateur a demandé même sans l'avoir écrite (« les
  utilisateurs ne listent jamais ces cas, ils les attendent »), avec l'exemple de la même
  personne écrite autrement ; le guide du début de tour et le relecteur le disent aussi, et
  « sauf si la demande le veut » devient « sauf si la demande veut explicitement l'autre
  résultat ». Mesures : rejeu du contrôle 12/12, essai complet 12/12 (tableau de t7).
- Suite (4 h 40) : un essai de confirmation fait 11/12 ; le modèle trouve une autre raison
  d'écarter la ligne, « le code existant ne le fait pas non plus ». Le contrôle et le guide
  disent maintenant qu'un ancien code qui saute un cas n'est pas une raison de le sauter dans
  le nouveau (le modèle le traite et signale l'ancien manque à l'utilisateur) : rejoué au même
  point, 12/12. Remesuré ensuite par deux essais complets (7 h 52) : 12/12 les deux fois, en
  37 et 39 requêtes.

### T1-33 Le modèle migre à la main le fichier de données au lieu de lire l'ancienne forme
- Preuve : t7 répété, avec et sans l'aide : 3/12 les deux fois. Pour ajouter les inscrits, le
  modèle ajoute `"inscrits": []` à chaque atelier de `donnees.json`, puis écrit
  `atelier["inscrits"]` sans valeur par défaut. Chez lui tout marche ; sur le fichier de
  l'utilisateur (la vérification remet celui de départ, comme toute autre copie : sauvegardes,
  autres postes), `liste` plante. La garde des données ne voyait pas les `.json` (pour ne pas
  viser `package.json` et les autres fichiers de configuration).
- État : corrigé (reprise du 13 octobre, 2 h 22). Un fichier `.json` du projet qui n'est pas
  une configuration connue (`package.json`, `tsconfig*.json`, `*.config.json`, fichiers et
  dossiers cachés, `node_modules`...) est vu comme les données de l'application. Quand le
  modèle le change avec ses outils, Claw compare au contenu du début du tour : une clé ajoutée
  à au moins deux entrées qui existaient (une migration ; une seule clé nouvelle est plutôt
  une entrée ajoutée) ou une valeur changée ou retirée est signalée au modèle, une fois par
  fichier : les copies de l'utilisateur gardent l'ancienne forme, le code doit la lire aussi
  (une valeur par défaut au chargement), et le fichier revenir comme il était, sauf demande
  de l'utilisateur. Mesure : t7 avec cette garde, 12/12 (essai complet, 56 requêtes).

### T1-34 Une correction qui change ce que renvoie une fonction
- Preuve : t3 refait avec la table revue (T1-28) : 2/4, contre 4/4 avant. Le mot « ventes »
  y déclenche maintenant aussi la ligne des montants, qui disait de garder les montants en
  `Decimal`, « never in floats ». Pour corriger le plantage, le modèle fait renvoyer des
  `Decimal` à `rapport.moyennes()` ; tout appelant qui fait `json.dumps` de son résultat
  plante (TypeError), comme la vérification cachée.
- État : corrigé (reprise du 13 octobre, 7 h 22). La ligne des montants dit de calculer en
  centimes ou en `Decimal`, mais de garder ce que renvoient les fonctions existantes
  (`json.dumps` refuse un `Decimal` : convertir là où les valeurs sortent du code), avec
  l'essai qui le montre ; l'étape 4 du débogueur dit de corriger par le plus petit changement
  qui suffit, en gardant ce que les fonctions existantes prennent et renvoient. Mesure : t3
  refait, 4/4 en 17 requêtes.

### T1-36 La garde des tests visait aussi les tests que le modèle venait d'écrire
- Preuve : dans les essais du 13 octobre (t1, t4, t5), la remarque « test_x.py sert à vérifier
  le travail et a été modifié après un échec » revient souvent. Sur t5 (21 h), les trois
  modifications gardées de `test_api.py` retiraient des lignes du test neuf du modèle ; deux
  seulement contenaient une ligne présente aussi dans le fichier d'origine
  (`assert statut == 200`). La garde regardait toute ligne retirée, et la remarque dit de
  « restaurer la vérification » : elle peut pousser le modèle à défaire la correction de son
  propre test.
- État : corrigé (reprise du 13 octobre, 20 h 55) : seules comptent les lignes qui existaient
  au début du tour, et seulement si le fichier en compte maintenant moins qu'alors (retirer une
  copie ajoutée par le modèle ne compte pas) ; un fichier de tests créé pendant le tour est au
  modèle. Le contenu du début du tour est relevé avant la première modification (comme pour
  T1-29).

### T1-35 Pas de relecture quand les relecteurs gratuits sont saturés
- Preuve : t4 du 13 octobre au matin (10/12) : laguna et dots ne finissent pas leur réponse
  dans le temps donné (240 s, puis les 120 s qui restaient) ; le travail rend la main sans
  relecture, et le prêt d'office de T1-18 passe. Le même jour, t7 (deux fois) et t4 ont aussi
  vu des relecteurs inachevés.
- État : corrigé (reprise du 13 octobre, 7 h 22) : si aucun relecteur ne répond, le modèle de
  la session relit lui-même, dans une conversation neuve (pas le raisonnement qui a écrit le
  code), avec les mêmes consignes. Vérifié de bout en bout avec le faux modèle (relecteur
  configuré introuvable, relecture par le modèle de la session). Le faux modèle répond
  maintenant aux demandes de relecture (reconnues à la consigne du relecteur).
- Limite mesurée : sur t4, ce relecteur de secours, sans la phrase citée (T1-18, suite), lit
  la même chose que l'agent (« auto-checkout to next in line ») ; c'est la phrase citée qui
  lui fait lire juste.

## Améliorations

Lot T1 (en cours le 12 octobre 2026), dans cet ordre : T1-08 (garde des données), T1-03 et
T1-04 (guide de débogage : hypothèse de l'utilisateur, essai au-delà de l'exemple), T1-05
(contexte Git borné), T1-09 (fichier montré après une modification), T1-10 (essais
regroupés), T1-07 (`/tmp` et Python du Store), T1-06 (réglages recopiés dans le prompt).
T1-01 est laissé tel quel : la demande de preuves coûte une requête mais a rendu la seconde
réponse du petit modèle mieux étayée, et l'utilisateur préfère la profondeur à la vitesse.
T1-02 est mineur (la consigne demande déjà de finir par le compte rendu).

Lot T1 fait le 12 octobre 2026 (commit « Lot T1 ») : 629 tests, vérification complète au
vert. Relecture contradictoire par deux agents avant le commit : 20 constats (1 grave, 8
moyens), 18 corrigés, 2 restés ouverts (T1-11, T1-12). Trois défauts qui existaient avant le
lot ont été corrigés au passage : remarques de `write_file` (contenu abrégé, codage) perdues
à l'allègement, garde des tests réécrits muette quand un hook ajoute du texte au résultat,
jetons des réglages envoyés dans le prompt.

Suite, dans l'ordre (mise à jour du 13 octobre, 21 h 30) : avec Claw à jour, le banc entier
tient (t1 6/6, t2 8/8, t3 4/4, t4 12/12, t5 12/12, t7 12/12 ; t6 12/12 le 12 au soir). (1)
après la remise à zéro du quota (2 h) : t6 avec le plus puissant et Claw à jour, et une
mesure de T1-36 (combien de remarques de la garde des tests restent) ; les répétitions de t7 ont montré qu'un seul essai ne
suffit pas (11/12 puis 3/12 pour le même témoin) ; (2) T1-24 sur un autre genre de fonctionnalité (montants de t1, CSV de t3) ;
(3) T1-18 : chercher ce qui fait voir à un modèle
la contradiction entre la phrase de l'utilisateur et sa propre sortie (le rejeu du contrôle
sur t4c sert de banc : une à deux requêtes par essai) ; (4) T1-11, les données réécrites par
une commande ; (5) T1-04, les espaces insécables oubliées sur t3 ; (6) refaire chaque mesure
deux ou trois fois avant de conclure. T1-03 à T1-17 et T1-19 à T1-36 sont faits, T1-18 en
partie (la relecture le rattrape quand la phrase lui est citée ; l'agent, lui, défend encore
son code). Le troisième modèle gratuit (laguna) fait 12/12 sur t4
et t5 avec le moins de requêtes (16 et 26), mais son fournisseur le limite souvent.

Mesure faite (tableau plus haut). Pour la refaire : `run.py <scénario> --label <nom>`, puis avec
`--model cohere/north-mini-code:free`) et comparer scores et requêtes à la série propre.
Ensuite : des scénarios plus durs (plusieurs fichiers, tâche longue) et un troisième modèle.
