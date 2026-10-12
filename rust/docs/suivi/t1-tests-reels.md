# Campagne T1 : essais réels avec de vrais modèles gratuits

Demande de l'utilisateur, le 12 octobre 2026 : « Fais des vrais tests sur Claw avec de vrai
models, et améliore le, qu'il prenne exemple sur comment les models Opus et Fable
réfléchissent. Fais toujours en sorte qu'on puisse reprendre sur une autre session. »

## Méthode

- Banc d'essai : `rust/scripts/real-tests/` (voir son README). Trois scénarios, chacun avec
  une vérification cachée qui note le résultat ; les essais tournent hors du dépôt, dans
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
- État : à faire

### T1-12 Après `/undo`, la page réaffiche la vue du fichier en entier
- Preuve : relecture du lot T1. Dans le fil rejoué, la vue jointe au résultat de la dernière
  modification de chaque fichier apparaît sous « Remarque de Claw au modèle » (jusqu'à 4 000
  caractères). À la réouverture d'une session ce n'est plus le cas (vues remplacées).
- Remède : replier la vue en une ligne dans la page.
- État : à faire

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
- État : à faire (premier point à reprendre)

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

Suite, dans l'ordre : (1) T1-13, la réponse finale réduite à un accusé de réception ;
(2) T1-11 et T1-12 ; (3) T1-04, les espaces insécables que les deux modèles oublient encore
sur t3 ; (4) des scénarios plus durs (plusieurs fichiers, tâche longue, projet web comme
ceux de l'utilisateur) et un troisième modèle gratuit, les trois scénarios actuels étant
presque tous réussis ; (5) refaire chaque mesure deux ou trois fois avant de conclure.

Mesure faite (tableau plus haut). Pour la refaire : `run.py <scénario> --label <nom>`, puis avec
`--model cohere/north-mini-code:free`) et comparer scores et requêtes à la série propre.
Ensuite : des scénarios plus durs (plusieurs fichiers, tâche longue) et un troisième modèle.
