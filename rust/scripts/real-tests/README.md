# Essais réels de Claw

Banc d'essai de Claw avec de vrais modèles gratuits, sur de petits projets où un modèle doit
raisonner comme un bon ingénieur : trouver une cause cachée, couvrir des exigences que la
demande ne dit pas en toutes lettres, vérifier une hypothèse de l'utilisateur au lieu de la
suivre. Chaque scénario a une vérification cachée (`check.py`), jamais copiée dans le projet,
qui note le résultat.

```bash
python run.py --list                         # scénarios et leur demande
python run.py t1-inventaire --label base     # un essai, avec le modèle réglé par l'utilisateur
python run.py t2-notes --model poolside/laguna-s-2.1:free --label laguna
```

Chaque essai :

- copie `scenarios/<nom>/project` dans `rust/target/real-tests/<date>-<nom>-<label>/project` ;
- lance `claw` sur cette copie, sans interaction, en mode `danger-full-access` (la copie seule
  est en jeu) et avec `CLAW_FALLBACK=0`, pour rester sur un seul modèle ;
- note les requêtes gratuites consommées (`claw credits` avant et après), les appels
  d'outils, les remarques de Claw et la réponse finale ;
- lance `check.py` et écrit `result.json` ; une ligne résume l'essai dans
  `rust/target/real-tests/results.jsonl`.

Règles : modèles gratuits seulement ; peu d'essais, ciblés (chaque étape de l'agent coûte une
requête du quota gratuit) ; les vraies clés sont lues par Claw et ne sont jamais affichées.
Les constats et les scores sont consignés dans `rust/docs/suivi/t1-tests-reels.md`.

| Scénario | Ce qu'il éprouve |
|---|---|
| `t1-inventaire` | deux causes à un même échec de tests (liste par défaut partagée, flottants comparés exactement) : chercher la vraie cause, ne pas affaiblir les tests |
| `t2-notes` | une fonctionnalité avec des exigences implicites (numéro d'origine, accents dans le mot cherché, appel sans mot, aide du module) |
| `t3-rapport` | l'utilisateur croit son fichier corrompu : vérifier l'hypothèse, corriger le code, garder les données intactes |
