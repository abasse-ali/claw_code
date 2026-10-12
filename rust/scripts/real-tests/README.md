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

- copie `scenarios/<nom>/project` dans `<essais>/<date>-<nom>-<label>/project` et en fait un
  dépôt Git à lui (un commit initial) ;
- lance `claw` sur cette copie, sans interaction, en mode `danger-full-access` (la copie seule
  est en jeu) et avec `CLAW_FALLBACK=0`, pour rester sur un seul modèle ;
- note les requêtes gratuites consommées (`claw credits` avant et après), les appels
  d'outils, les remarques de Claw au modèle, ses notices (vérification, relecture faite,
  approuvée ou en échec, limite de requêtes) et la réponse finale ;
- garde chaque réponse du relecteur, approbation comprise (`reviews.jsonl`, par
  `CLAW_REVIEW_LOG`) ;
- lance `check.py` et écrit `result.json` ; une ligne résume l'essai dans
  `<essais>/results.jsonl`.

`python review_replay.py <essai> [--model M] [--baseline COMMIT]` rejoue la relecture par un
autre modèle sur le travail d'un essai déjà fait (demande, diff du projet, réponse finale) : une
requête, aucun outil. Avec `--baseline`, ce sont les consignes de relecture de ce commit, pour
comparer deux formulations sur le même travail ; `--reviewer` impose le relecteur (un modèle
gratuit). Le résultat s'ajoute à `<essai>/review-replay.jsonl`.

`python checklist_replay.py <essai> --cut N [--baseline COMMIT] [--restore FICHIER]` rejoue le
contrôle des exigences de Claw au point exact où il était arrivé dans la session de l'essai
(message N) : la session est coupée là, le contrôle est renvoyé au vrai modèle (formulation
actuelle, ou celle d'un commit), qui reprend avec tout ce qu'il savait alors. Une à deux
requêtes au lieu d'un essai entier ; le résultat est noté par la vérification cachée.

`<essais>` est hors du dépôt : `CLAW_REAL_TESTS_DIR`, sinon le dossier `claw-real-tests` à côté
du dépôt (`D:\Mes_Projets\claw-real-tests`). Pas dans `%LOCALAPPDATA%` : le Python du
Microsoft Store y écrit dans un dossier virtuel que Git et Claw ne voient pas. Dans le dépôt,
Claw donnerait au modèle l'état et le
diff Git du dépôt entier (le 12 octobre, un petit modèle y a lu les pièges d'un scénario), et
les `CLAW.md`, `CLAUDE.md` ou `AGENTS.md` des dossiers parents : `run.py` refuse un dossier
d'essais qui en a au-dessus de lui.

Règles : modèles gratuits seulement ; peu d'essais, ciblés (chaque étape de l'agent coûte une
requête du quota gratuit) ; les vraies clés sont lues par Claw et ne sont jamais affichées.
Les constats et les scores sont consignés dans `rust/docs/suivi/t1-tests-reels.md`.

| Scénario | Ce qu'il éprouve |
|---|---|
| `t1-inventaire` | deux causes à un même échec de tests (liste par défaut partagée, flottants comparés exactement) : chercher la vraie cause, ne pas affaiblir les tests |
| `t2-notes` | une fonctionnalité avec des exigences implicites (numéro d'origine, accents dans le mot cherché, appel sans mot, aide du module) |
| `t3-rapport` | l'utilisateur croit son fichier corrompu : vérifier l'hypothèse, corriger le code, garder les données intactes |
| `t4-bibliotheque` | tâche plus longue sur plusieurs modules : ajouter les réservations (file d'attente, livre mis de côté au retour, persistance, ligne de commande) ; 12 points, dont cinq exigences que la demande ne dit pas |
| `t6-conges` | des dates : jours ouvrés d'un congé, dates écrites à la française, fériés lus dans un fichier, décompte du solde ; 12 points, dont sept exigences que la demande ne dit pas (bornes comprises, « 01/04 » est le 1er avril, férié un samedi, année bissextile, fin avant début, date impossible, solde insuffisant) |
| `t5-recettes-web` | projet web sans dépendance (serveur Python, API JSON, page HTML et JavaScript) : une recherche dans l'API et un champ qui filtre la page pendant qu'on tape ; la vérification lance le serveur comme l'utilisateur et l'interroge ; 12 points, dont sept exigences que la demande ne dit pas (accents et majuscules, plusieurs mots, recherche vide, aucune correspondance, tri, encodage de l'URL) |
