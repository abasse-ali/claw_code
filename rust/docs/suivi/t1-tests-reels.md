# Campagne T1 : essais réels avec de vrais modèles gratuits

Demande de l'utilisateur, le 12 octobre 2026 : « Fais des vrais tests sur Claw avec de vrai
models, et améliore le, qu'il prenne exemple sur comment les models Opus et Fable
réfléchissent. Fais toujours en sorte qu'on puisse reprendre sur une autre session. »

## Méthode

- Banc d'essai : `rust/scripts/real-tests/` (voir son README). Trois scénarios, chacun avec
  une vérification cachée qui note le résultat ; les essais tournent dans
  `rust/target/real-tests/` (hors du suivi Git), une ligne par essai dans `results.jsonl`.
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
(`%LOCALAPPDATA%\claw-real-tests`), chaque projet dans son propre dépôt Git, et refuse un
dossier qui a des instructions ou un dépôt au-dessus de lui. Tous les essais sont à refaire
dans ces conditions.

## Constats

### T1-01 La vérification des preuves coûte une requête quand la session montre déjà l'avant et l'après
- Preuve : t1 base (messages 7 à 22) et t3 base (messages 4 à 12) : le modèle a lancé la commande
  en échec, corrigé, relancé la même commande avec succès ; la remarque « Before this answer
  goes to the user: they reported a problem... » part quand même, et le modèle rend la même
  réponse.
- Remède : ne pas l'envoyer quand une même commande a échoué puis réussi après la dernière
  modification (cause observée, disparition observée).
- État : à faire

### T1-02 Après le contrôle des exigences, la réponse finale devient un tableau d'audit
- Preuve : t2 base, message 29 : « Vérification de chaque exigence » suivi d'un tableau ;
  le compte rendu destiné à l'utilisateur (message 22 : ce qui a été ajouté, comment
  l'utiliser) est remplacé, et c'est le dernier message que l'utilisateur lit.
- Remède : la remarque du contrôle demande de finir par le compte rendu pour l'utilisateur
  (résultat, ce qui a été fait, vérification), la vérification des exigences n'y entrant
  qu'en résumé.
- État : à faire

### T1-03 L'hypothèse de l'utilisateur n'est ni vérifiée ni démentie dans la réponse
- Preuve : t3 base : « Je pense que le fichier ventes.csv est corrompu, tu peux le réparer ? ».
  Le modèle a bien corrigé le code, mais sa réponse ne dit jamais que le fichier n'est pas
  corrompu ni qu'il n'a pas été modifié (R1, R7).
- Remède : le guide de débogage dit qu'une cause avancée par l'utilisateur est une hypothèse
  à confirmer ou réfuter avec une preuve, et que la réponse le lui dit.
- État : à faire

### T1-04 Le correctif n'est essayé que sur les données de l'exemple
- Preuve : t3 base : `replace(" ", "")` ; les espaces insécables des exports Excel en
  français (U+00A0, U+202F) font encore échouer la lecture ; aucun essai sur d'autres valeurs.
  La réponse conclut « Rien d'autre à faire » (R5, R6).
- Remède : à préciser après les essais avec des modèles plus petits.
- État : à étudier

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
- État : à faire

## Améliorations

(à compléter : ce qui a été changé dans Claw, et l'effet mesuré sur les mêmes scénarios)
