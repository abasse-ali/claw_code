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

## Constats

(à compléter : T1-01, T1-02, ... avec preuve, défaut, remède, état)

## Améliorations

(à compléter : ce qui a été changé dans Claw, et l'effet mesuré sur les mêmes scénarios)
