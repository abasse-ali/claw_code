# E2. Sessions réelles : où le modèle se trompe

Question : dans les sessions réelles du projet `test_claw`, à quels moments le modèle gratuit
s'est-il trompé ou a-t-il perdu des étapes, qu'a fait Claw à ce moment, et que pourrait-il faire
de plus sans appeler un autre modèle ?

La session du 11 octobre 2026 (`session-1791665810709.json`) a déjà été étudiée : ses leçons
sont dans les commits f637bf7 et 5260b7a. Cette étude porte sur les deux autres.

Chaque constat est ajouté ici dès qu'il est confirmé, sous cette forme :

```
## E2-nn Titre court
- Gravité : haute | moyenne | basse
- Session et messages : fichier, numéros de messages
- Ce qui s'est passé : les faits, avec le nombre d'étapes perdues
- Ce que Claw a fait : la remarque envoyée, ou rien
- Remède : ce que Claw pourrait faire, de préférence sans requête de plus
- État : à vérifier
```

« État » passe à « confirmé », « écarté » ou « corrigé (commit) » quand la session principale
le traite.

