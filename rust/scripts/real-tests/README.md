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

`python rescore.py <essai>` rejoue la vérification cachée d'un essai déjà fait, sans requête au
modèle (vérification corrigée ou rapport illisible) : la note et la ligne de `results.jsonl`
sont refaites.

`python review_replay.py <essai> [--model M] [--baseline COMMIT]` rejoue la relecture par un
autre modèle sur le travail d'un essai déjà fait (demande, diff du projet, réponse finale) : une
requête, aucun outil. Avec `--baseline`, ce sont les consignes de relecture de ce commit, pour
comparer deux formulations sur le même travail ; `--reviewer` impose le relecteur (un modèle
gratuit). Le résultat s'ajoute à `<essai>/review-replay.jsonl`.

`python checklist_replay.py <essai> --cut N [--baseline COMMIT] [--restore FICHIER]` rejoue le
contrôle des exigences de Claw au point exact où il était arrivé dans la session de l'essai
(message N) : la session est coupée là, le contrôle est renvoyé au vrai modèle (formulation
actuelle, ou celle d'un commit), qui reprend avec tout ce qu'il savait alors. Une à deux
requêtes au lieu d'un essai entier ; le résultat est noté par la vérification cachée. Les
chemins de l'essai d'origine sont réécrits vers le rejeu (sinon le modèle essaie son code dans
l'ancien projet), et le guide des cas limites du début de tour n'est pas joint une seconde fois.

`<essais>` est hors du dépôt : `CLAW_REAL_TESTS_DIR`, sinon le dossier `claw-real-tests` à côté
du dépôt (`D:\Mes_Projets\claw-real-tests`). Pas dans `%LOCALAPPDATA%` : le Python du
Microsoft Store y écrit dans un dossier virtuel que Git et Claw ne voient pas. Dans le dépôt,
Claw donnerait au modèle l'état et le
diff Git du dépôt entier (le 12 octobre, un petit modèle y a lu les pièges d'un scénario), et
les `CLAW.md`, `CLAUDE.md` ou `AGENTS.md` des dossiers parents : `run.py` refuse un dossier
d'essais qui en a au-dessus de lui.

Sur GitHub, le workflow « Essais réels » (`.github/workflows/real-tests.yml`, lancé à la
main) fait tourner le banc avec un modèle de NVIDIA (secret `NVIDIA_API_KEY` du dépôt) :
scénarios au choix (« tous » pour le banc entier). Claw est compilé une fois, puis chaque
scénario a son job, trois à la fois par défaut (`parallel`), pour rester sous la limite de
requêtes de NVIDIA. Lancé sur une branche (`ref`), il essaie le Claw de cette branche.
`report.py` écrit dans le journal du job la note de chaque point, les notices et les
remarques de Claw, la réponse finale et la trace de la session ; la clé est retirée des
fichiers des essais avant leur dépôt en artefact.

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
| `t7-ateliers` | inscriptions à des ateliers à places limitées, données en JSON, sans doublon ; 12 points, dont sept exigences que la demande ne dit pas (ancien fichier sans inscrits, atelier complet qui ne change rien, même personne écrite autrement, atelier inconnu, nom accentué, atelier sans place, aide à jour). Fait pour mesurer les genres nouveaux des cas limites, avec un témoin `CLAW_EDGE_CASES=0` |
| `t8-materiel` | créer un projet complet dans un dossier vide (prêts de matériel d'une association, en ligne de commande, données en JSON à côté du programme, tests, README) ; la vérification travaille dans une copie sans données, comme un utilisateur qui vient de l'installer ; 12 points, dont sept exigences que la demande ne dit pas (plus d'exemplaire disponible, matériel inconnu, retour d'un prêt inexistant, saisies invalides, retard compté le lendemain, lancement depuis un autre dossier, sortie dans un tube sans UTF-8 forcé). Fait pour mesurer le mode projet, avec un témoin `CLAW_PROJECT_MODE=0` ; projet intact 0/12, référence 12/12 |
| `t5-recettes-web` | projet web sans dépendance (serveur Python, API JSON, page HTML et JavaScript) : une recherche dans l'API et un champ qui filtre la page pendant qu'on tape ; la vérification lance le serveur comme l'utilisateur et l'interroge ; 12 points, dont sept exigences que la demande ne dit pas (accents et majuscules, plusieurs mots, recherche vide, aucune correspondance, tri, encodage de l'URL) |
