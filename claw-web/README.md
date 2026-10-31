# Claw web

Appli web de [Claw](https://github.com/abasse-ali/claw_code) (dossier `claw-web` du dépôt du
code de Claw), l'agent de code qui travaille avec
des modèles gratuits : choisissez un dépôt GitHub, une branche et un modèle, décrivez une tâche ;
Claw y travaille dans GitHub Actions, sur la branche de la session, la pousse et vous répond.
Installable sur le téléphone (« Ajouter à l'écran d'accueil »).

Adresse : https://abasse-ali.github.io/claw_code/ (publiée par le workflow `pages.yml` à chaque
changement de ce dossier ; l'ancienne adresse, https://abasse-ali.github.io/claw-web/, y renvoie).

## Comment ça marche

- Cette appli est une page statique, sans serveur : elle parle directement à l'API de GitHub
  (et à la liste publique des modèles d'OpenRouter), avec votre jeton.
- Le travail se fait dans le **dépôt du moteur**, un dépôt privé (par défaut
  `abasse-ali/claw-sessions`) : son workflow `claw-web.yml` appelle le moteur, workflow du dépôt
  public du code de Claw, qui lance Claw sur le dépôt de la tâche, sur la branche de la session,
  puis la pousse. Chaque session est une branche à part entière (`claw/session-<n>`), partie de
  la branche choisie : aucune pull request, rien n'est fusionné sans votre demande. Les journaux
  et les minutes de GitHub Actions restent dans le dépôt privé.
- Claw garde une **mémoire par projet** dans le dépôt du moteur (branche `claw-web-fichiers`,
  `memoire/<propriétaire>/<dépôt>.md`) : les leçons qu'il note, les erreurs déjà rencontrées et
  leur issue, les commandes qui ont marché, les dernières demandes. Il la relit au début de
  chaque tâche sur ce dépôt ; effacer le fichier la remet à zéro.
- Option **Mode projet** (menu des options, coupée par défaut) : pour créer une application de
  A à Z, Claw écrit un cahier des charges et un plan en jalons, puis les fait un à un, vérifiés.
  Coupée, il répond à la demande telle quelle. Une simple question reçoit sa réponse, sans
  toucher aux fichiers.
- Chaque session est une issue du dépôt du moteur (étiquette `claw-web`) : la tâche, vos
  messages de suivi, la progression en direct et les réponses de Claw. Le dépôt étant privé,
  elles ne sont visibles que de vous.

## Mise en route

1. Créez un [jeton à grain fin](https://github.com/settings/personal-access-tokens/new) :
   - dépôts : le moteur et ceux où Claw travaillera (ou tous) ;
   - autorisations des dépôts, en lecture et écriture : **Actions**, **Contents**, **Issues**,
     **Pull requests**, **Secrets** (Metadata en lecture vient avec) ;
   - pour mettre un site en ligne depuis l'appli (« Mettre en ligne », GitHub Pages) :
     **Pages** en lecture et écriture ;
   - pour créer des dépôts depuis l'appli (« Créer un dépôt » dans le choix du dépôt) :
     **All repositories** et **Administration** en lecture et écriture ;
   - pour le serveur gratuit et son robot de réessai : **Administration** et **Variables** en
     lecture et écriture, sur le dépôt des sessions et celui du code de Claw.

   Modifier ces réglages garde la valeur du jeton : rien à refaire dans l'appli.
2. Ouvrez l'appli, collez le jeton, enregistrez.
3. Dans « Moteur », « Clés des modèles » : collez votre clé OpenRouter (ou laissez le champ
   vide si elle est déjà gardée sur l'appareil), laissez activé « Donner le jeton au moteur »,
   puis « Configurer le moteur ». L'appli pose deux secrets dans le dépôt du moteur,
   chiffrés dans le navigateur : `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN`. Facultatif, deux
   autres fournisseurs gratuits, dont les clés ne sont jamais gardées sur l'appareil :
   - une clé NVIDIA (`nvapi-…`, créée sur build.nvidia.com, profil, Settings, API Keys) devient
     le secret `NVIDIA_API_KEY` : le choix du modèle propose les agents de NVIDIA, sans quota par
     jour annoncé, et Claw fait décrire les images par le lecteur de NVIDIA ;
   - une clé Google Gemini (`AIza…`, créée sur aistudio.google.com/apikey, dans un projet sans
     facturation, sinon chaque requête est payante) devient le secret `GEMINI_API_KEY` : le
     choix du modèle propose les modèles de l'offre gratuite de Google (quota par jour et par
     modèle, bas pour les plus récents).
   Une clé collée dans le mauvais champ est refusée avant tout envoi.
4. Choisissez un dépôt et une branche, ou créez un dépôt neuf, puis décrivez la tâche. Dans un
   dépôt neuf, une demande de création (« Crée une application... ») fait passer Claw en mode
   projet : cahier des charges, plan en jalons vérifiés, démonstration finale. Les messages
   de suivi le poursuivent (une nouvelle session aussi, quand `SPEC.md` et `PLAN.md` sont dans
   le dépôt) ; « sans mode projet » l'arrête pour la session.

## Supprimer un dépôt ou une branche

- **Dépôt** : la corbeille d'un dépôt, dans le choix du dépôt. Il faut retaper son nom complet,
  comme sur GitHub ; les sessions qui y travaillent sont annoncées. GitHub permet de le
  restaurer pendant 90 jours (Réglages de GitHub, Repositories, Deleted repositories). Le jeton
  doit avoir l'autorisation Administration en lecture et écriture. Le dépôt du moteur et le code
  de Claw ne sont jamais proposés : Claw Web en dépend.
- **Branche** : la corbeille d'une branche, dans le choix de la branche (jamais la branche par
  défaut). La session qui y travaille est nommée ; son prochain passage recrée la branche à
  partir de la branche de départ.

## La conversation

Vos messages sont en bulle à droite ; les réponses de Claw, sans cadre, sur toute la largeur,
avec leurs titres, et les remarques du moteur (travail inachevé, par exemple) en encadré.

Un passage en cours a son bloc « Claw travaille » : les griffes du logo qui grattent, la durée à
la seconde et « Arrêter » ; « En ce moment », l'étape en cours en grand (la tâche que Claw fait,
sinon ce qu'il annonce, sinon sa dernière action) avec une barre découpée par tâche (« 2 sur 5 »)
et son plan à déplier ; ses quatre dernières actions, la plus récente en haut, les nouvelles
entrant en glissant l'une après l'autre ; le journal complet, replié. « Claw se prépare » (file
d'attente, compilation) prend la même forme, et l'arrêt demandé le fait virer au rouge
(« Arrêt en cours »). Fini, un passage tient en une carte : son état (fini, arrêté, erreur), sa
durée, sa frise (une barre par action, de la couleur de sa pastille dans le journal, rouge pour
une action qui a échoué) avec son nombre d'actions, les fichiers modifiés (+ −) et les
commandes ; un toucher déplie son journal (le fil se trace, les étapes arrivent en cascade).

Le journal commence par ce que le passage a fait, d'un coup d'œil : fichiers modifiés (avec leurs
lignes ajoutées et retirées, « nouveau » pour un fichier créé), commandes, captures, lectures,
commandes en échec et défauts que montre encore la dernière capture. Le détail suit, rangé par
tâche de Claw (« Préparation », puis chaque tâche de sa liste, puis « Vérification et réponse ») :
une carte par tâche, avec son état et son nombre d'actions, repliée sauf la dernière ; sans liste
de tâches, les 30 dernières étapes sont en vue. Chaque action est une phrase simple, avec sa
pastille de couleur sur une frise (« Lit index.html », « Modifie styles.css », « Lance une
commande », « Regarde la page ») ; les annonces de Claw aussi (« Page vérifiée : 2 défauts
réparés, encore 3 à corriger », « Laguna S 2.1 ne voit pas les images : un autre modèle lui
décrit 2 captures »), et une vérification de la page qui ne change rien ne se répète pas. Les
réflexions de l'agent (souvent en anglais) sont cachées : « Voir ses réflexions » les montre,
choix gardé sur l'appareil ; ce qu'il se dit en anglais en travaillant tient en une ligne
« Note de l'agent », à déplier. Ses messages en français sont en bulles, ses griffes dans la
pastille. Les lectures (ou recherches) qui se suivent tiennent en une ligne, leur nombre à côté
du verbe (« Lit 3 index.html, styles.css… »), et se déplient sur chacune. Un long journal a ses
filtres (« Messages », « Modifications », « Commandes », « Lectures », « Web », « Outils », avec
leur nombre ; toucher encore la puce remet tout) : les tâches concernées s'ouvrent, les autres
se retirent, et le filtre reste quand la session se redessine. Au pied, la fiche du passage :
modèle, requêtes, durée (et coût), puis les requêtes gratuites du jour.

Dans une session, la flèche « Revenir en bas » paraît à droite quand la fin n'est plus à l'écran ;
si Claw avance pendant que vous lisez plus haut, la page reste où elle est et la flèche devient
« Nouvelles actions ».
Les tâches de Claw restent dans un petit bloc pliable au-dessus de la saisie (caché tant que le
bloc « Claw travaille », qui les montre déjà, est à l'écran), ses questions dans le même bloc, en
cartes à faire défiler à gauche et à droite. Sous une réponse, les fichiers modifiés sur la branche et
l'exécution sur GitHub sont en pastilles, le modèle et les requêtes à déplier. « Relancer » et
« Mettre en ligne » restent sous la conversation ; l'exécution et la session sur GitHub,
« Archiver » et « Supprimer » sont dans le menu « ••• ».

Votre message paraît dès son envoi, et une nouvelle session s'ouvre dès sa création ; jusqu'à
la première progression, la carte « Claw se prépare » dit l'étape en cours (file d'attente,
compilation de Claw, préparation du dépôt). GitHub met quelques secondes à lister un passage
lancé : la session l'attend, au lieu de dire le message pas envoyé. « Ce message n'a pas encore
été envoyé à Claw » ne paraît que si le lancement a vraiment échoué (la raison s'affiche) ;
« Lancer le passage » ne relance pas un passage déjà parti. Claw n'est compilé qu'après une mise
à jour de son code Rust, une fois par type de processeur (le serveur ARM, le PC ou les machines
de GitHub) : quelques minutes, puis il est gardé en cache.

Une réponse que les contrôles de Claw lui font reprendre (une commande conseillée jamais
lancée, par exemple) est repliée dans le journal dès que Claw la reprend (« Réponse
provisoire »), sous la réponse définitive : le moteur la marque (pour un journal d'avant le
21 octobre 2026, l'appli la reconnaît à son texte). Claw, renvoyé au travail, redit d'abord en
une phrase ce qu'il fait. Un appel d'outil que le modèle a écrit en texte n'est pas montré.

Quand quelque chose d'essentiel n'est pas clair (quelle page, quelle version), Claw pose ses
questions avec des réponses proposées, comme Claude Code : le passage s'arrête sur elles, et un
toucher sur une réponse l'envoie (plusieurs questions : une réponse chacune, puis « Envoyer
les réponses ») ; on peut aussi répondre avec ses mots. Dans le journal, une capture d'écran dit
ce qu'elle montre : « Capture en ligne » (le site publié) ou « Capture de la session » (les
fichiers de la branche, pas encore en ligne).

## L'accueil

Un tableau de bord : la date et une salutation selon l'heure, trois chiffres (sessions au
travail, passages réussis sur 7 jours, modèle choisi, qu'un toucher change), puis la carte
« Espace de travail » : le dépôt et la branche où Claw travaillera, à changer d'un toucher, avec
ses fichiers et « Nouveau dépôt ». Le dépôt ne se choisit plus près du champ de saisie : le champ
dit pour quel dépôt on écrit (« Une tâche pour budget-app ? »). Viennent ensuite « En ce
moment », les sessions au travail avec leur durée à la seconde (l'accueil se relit toutes les
20 s tant qu'il y en a), « Pour démarrer », des demandes toutes prêtes qu'un toucher met dans le
champ (créer une appli, `/fix`, `/review`, `/verify`, `/jtest`, `/pages`), et les sessions
récentes en frise, par jour (« Tout voir » ouvre le menu).

## Apparence

Fond d'encre éclairé d'une aurore orange, rose et violette (thème clair au choix, ou celui du
téléphone : Réglages, « Apparence ») ; barre du haut, zone de saisie, fenêtres et tiroir en verre
dépoli ; un dégradé orange → rose → violet pour l'essentiel : l'envoi, les boutons principaux, le
logo, le contour d'un passage en cours. Titres en Bricolage Grotesque (police libre, licence
OFL, rangée dans `fonts/` : la page ne charge rien d'ailleurs), texte dans la police du téléphone.

Les animations sont courtes et ne jouent qu'à l'arrivée d'un élément ou sous le doigt : une page
qui arrive glisse en place, les listes entrent l'une après l'autre, les fenêtres montent du bas
et y redescendent, le tiroir glisse depuis la gauche, un message envoyé ou une réponse arrivée
entre dans la conversation. Une session redessinée en direct (toutes les 4 s pendant un passage)
ne rejoue rien : les roues d'attente et le contour du passage en cours reprennent où ils en
étaient. « Réduire les animations » (Réglages du téléphone, Accessibilité) coupe tout mouvement.

## Commandes « / » et niveau de réflexion

Un « / » en début de message (ou le bouton carré barré à côté des options) ouvre la liste des
commandes ; la suite du mot la filtre, un toucher ou Entrée la met dans le champ :

- les commandes de Claw, que le moteur déplie en consignes comme `claw /review` dans un
  terminal : `/review`, `/fix <ce qui ne marche pas>`, `/verify`, `/simplify`, `/audit`,
  `/security-review`, `/archi <sujet>`, `/avis <question>` ;
- celles de l'appli : `/jtest` (essayer sur le téléphone), `/pages` (mettre le site en ligne),
  `/merge` (fusionner la pull request), `/continue`.

La pastille « Réflexion », à côté du modèle, règle le niveau de réflexion du prochain message :
Auto (celui de Claw), Aucune, Faible, Moyenne, Élevée ou Maximale. Le choix reste pour les
messages suivants ; il est noté dans le message (une marque invisible), et le moteur le donne à
Claw (`CLAW_REASONING`). Au-dessus de Maximale, **Ultracode** (comme `/effort ultracode` dans
Claude Code) : la réflexion maximale, et le déroulé ultracode en plus. Un agent neuf étudie la
demande avant le travail, un vérificateur indépendant contrôle le résultat. C'est le même
interrupteur que celui du menu ✦ : l'un allume l'autre, et choisir un autre niveau le coupe.

## Aides de Claw : essayer un modèle seul

Autour du modèle, Claw vérifie, guide et recadre son travail : ses « aides au raisonnement »
(voir `rust/README.md`). Réglages, « Aides de Claw » (ou la pastille « Sans aides » de la saisie)
les montre toutes, 42 en six familles (vérifier le travail, guider le modèle, le remettre sur les
rails, protéger les fichiers, contexte et outils, appeler d'autres modèles), chacune avec ce
qu'elle fait et son interrupteur, « Tout couper » et « Tout rallumer ».

Demandé le 29 octobre 2026 : elles sont **toutes coupées** tant que rien n'est choisi, pour voir
chaque modèle seul, puis les rallumer une à une et garder celles qui lui vont. Le choix part avec
chaque message (une marque invisible) ; le moteur coupe les autres (`CLAW_VERIFY=0`…, pas
`CLAW_HARNESS=0`, qui couperait aussi les options de la saisie), et chaque réponse dit à son pied
avec quelles aides elle a été faite (« Sans aides », « 3 aides sur 42 », leurs noms avec le modèle
et les requêtes). Toutes allumées, la pastille disparaît et le message part sans marque : ce sont
celles de Claw. Le mode projet, le mode équipe et Ultracode restent dans le menu ✦ ; la lecture des
images pour un modèle qui ne les voit pas et le passage à un autre modèle en panne restent actifs.

Claw Web ne consulte d'habitude aucun autre modèle (`CLAW_ADVISOR_MODELS=off`,
`CLAW_ESCALATION_MODEL=off`) : « Conseil et second avis » cochée rallume les autres modèles,
« Escalade » le modèle qui reprend la main, pour le passage qui les a choisies. La « Relecture à
froid » n'en a pas besoin : le modèle de la session relit lui-même ses modifications, dans une
conversation neuve (demandé le 29 octobre 2026 : attendre les relecteurs gratuits prenait des
minutes).

## Ce que Claw compte faire

Pendant qu'il travaille, la progression de la session commence par **Ce que Claw compte faire** :
le plan que Claw annonce, en français, avant ses premiers outils. Elle se poursuit par **En
ce moment**, l'étape en cours. Le plan reste une fois la session finie. Un modèle qui se met au
travail sans un mot (Nemotron 3 Super ne met son plan que dans son raisonnement) reçoit une requête
sans outils : il ne peut y répondre que par ce plan, puis le travail reprend. Il en reçoit une
autre après 20 étapes muettes, pour dire où il en est. Pour NVIDIA, le choix du modèle propose
Nemotron 3 Super (recommandé), Nemotron 3 Ultra et Nemotron 3.5 Lightning ; un modèle de NVIDIA
indisponible passe la main au suivant, dans cet ordre. Step 5 Preview n'est plus proposé : en
secours de Super (introuvable chez NVIDIA le 20 octobre 2026), il a écrit de faux messages de
l'utilisateur et suivi les tâches qu'il y inventait. Claw écarte maintenant toute réponse de ce
genre avant de la montrer ou de l'exécuter.

## Photos trouvées par Claw

Quand l'agent cherche des photos (outil `Photo`), le journal du passage les montre en petites
vignettes sous la ligne « Photos », même celles qu'il écarte ensuite. Un toucher ouvre la photo en
grand, avec son titre, son auteur, sa licence et un lien vers sa page. Le moteur télécharge la
petite version chez la source (Flickr, Wikimedia Commons) et la glisse dans le journal ; seule la
grande version est chargée chez la source, la seule image extérieure que la page autorise.

Les captures d'écran de l'agent (outil `Screenshot`) s'affichent de la même façon : une vignette
du premier écran sous la ligne « Capture de la page », avec une pastille rouge quand des
problèmes y ont été relevés. Un toucher ouvre la capture entière (jusqu'à trois écrans de haut), à
faire défiler. Le moteur range ces captures sur la branche `claw-web-fichiers` du dépôt des
sessions, dans `uploads/session-<numéro>/`, que l'appli efface avec la session ; il lui faut pour
cela le droit d'écriture sur ce dépôt (`contents: write` dans le workflow de `claw-sessions`,
jamais donné à l'agent). Sans ce droit, seule la vignette reste.

## Réponses lisibles

La réponse de Claw est un compte rendu pour une personne : étiquettes en français (Résultat, Ce
qui n'allait pas, Ce que j'ai fait, Vérification, Ce qu'il reste à faire), les changements tels
que vous les verrez, sans liste de sélecteurs CSS ni de noms de fonctions, et sans ligne de
signature (une ligne « Assisted-by » finale est retirée par le moteur). Les listes imbriquées
s'affichent en retrait. Le pied de la réponse dit le bilan de la branche en français (« 10
fichiers modifiés, +1 743 lignes, −287 lignes »), et une erreur est d'abord dite en clair (« le
quota du modèle est épuisé pour le moment »), le message brut en dessous.

Deux messages courts ont un traitement à part :

- **Une question sur le travail** (« Tu fais quoi !? », « Pourquoi tu as fait ça ? », « Où en
  es-tu ? ») reçoit une réponse, et rien d'autre : Claw ne reprend aucun travail dans ce passage.
- **« Continue » dans une session neuve**, sans conversation qui précède : rien ne dit quoi
  continuer. Claw regarde le dépôt sans rien modifier, dit où en est le projet et propose la
  suite ; vous répondez avec ce qu'il faut faire. Vu dans la session 13 : un tel « Continue »
  avait lancé une refonte du site que personne n'avait demandée. Pour poursuivre un travail,
  écrivez « continue » dans la session où il a commencé.

## Quota du modèle choisi

L'anneau du haut suit le fournisseur du modèle choisi :
- **OpenRouter** (et « Auto ») : les requêtes gratuites restantes du jour, lues chez OpenRouter
  avec la clé gardée sur l'appareil.
- **Google Gemini** : la limite par jour du modèle quand elle est relevée (« 20/jour » pour
  Gemini 3.8 Flash). Google ne donne pas le compte en direct ; la consommation exacte est sur
  AI Studio, Rate limit.
- **NVIDIA** : les requêtes encore libres dans la minute, sur environ 40 (« 28/min »), en
  direct : le moteur note dans le journal l'heure de chaque requête de Claw, et l'appli compte
  celles de la dernière minute dans les sessions en cours (la session ouverte, ou les autres
  relues toutes les 20 s). L'anneau se remplit à mesure que la minute glisse ; « 40/min » quand
  rien ne tourne. Pas de quota par jour.

Le détail (toucher l'anneau) montre les trois fournisseurs, celui du modèle choisi d'abord ; le
choix du modèle donne le quota de chaque onglet ; chaque réponse de Claw dit combien de requêtes
son passage a prises, chez le fournisseur du modèle.

## Sessions sur votre machine : serveur gratuit ou PC

Réglages, « Sessions » : « Ma machine » fait tourner les sessions sans minutes GitHub
Actions, sur un serveur gratuit d'Oracle Cloud allumé jour et nuit, ou sur votre PC dans Docker
Desktop ; une session part sur la machine libre. La ligne « Ma machine » dit celles qui sont
en ligne ; les vérifications du moteur (« Moteur prêt », à déplier) montrent chaque exécuteur,
en ligne ou non, et en session.

- **Serveur gratuit** (offre « Always Free » d'Oracle : 2 cœurs ARM, 12 Go de mémoire, 200 Go
  de disque, sans limite de durée), tout depuis le téléphone : « Serveur gratuit dans le nuage »
  donne les étapes et prépare le script de démarrage à coller chez Oracle, avec un jeton
  d'inscription valable une heure (il faut l'autorisation Administration en écriture du dépôt des
  sessions). Le serveur installe seul Docker et l'exécuteur, en ligne au bout de 15 à 25 minutes.
- **PC** : `.github/claw-web/pc/claw-pc.ps1 install` dans le code de Claw. Le PC éteint, la
  session attend dans la file de GitHub et part dès qu'il revient.
- **Robot de réessai** : quand Oracle manque de place (« Out of capacity »), la même fenêtre
  démarre un robot qui réessaie toutes les 10 minutes, gratuitement sur GitHub, puis installe Claw
  dès qu'une machine se libère, et s'éteint seul. Il lui faut une clé d'API d'Oracle (aperçu de
  la configuration et fichier `.pem`), posée chiffrée en secret du dépôt du code de Claw, et le
  jeton de l'appli avec les autorisations Variables et Administration (l'appli les vérifie avant
  de poser les secrets, et nomme celle qui manque).

Détails, commandes et limites (machine inactive sept jours arrêtée par Oracle, « Out of capacity
») : `.github/claw-web/pc/README.md`.

## Outils MCP

Pendant une session, Claw a les outils de Claude Code sur le web, servis par le moteur sous forme
de trois serveurs MCP (liste complète dans le menu, « Outils MCP », avec un filtre) :

- **`github`** (54 outils, mêmes noms et paramètres que le serveur MCP officiel de GitHub) :
  issues et leurs commentaires, sous-issues, pull requests, revues et fils de revue, branches,
  commits, fichiers, Actions (passages, journaux des jobs, relance), vérifications (check runs),
  versions, étiquettes, recherche, recherche de secrets dans un texte. Le moteur fait chaque
  appel avec le jeton, que l'agent n'a jamais, et **seulement sur le dépôt de la session** (les
  outils de découverte, comme `get_me` ou `search_repositories`, mis à part). Restent à
  l'utilisateur : la fusion d'une autre pull request que celle de la session (celle de la session
  est fusionnée après la poussée, quand vous le demandez, comme `claw-github merge`), la création
  ou la copie d'un dépôt (« Créer un dépôt » dans le choix du dépôt), et l'écriture de fichiers
  par l'API sur la branche de la session ou sur la branche de départ : Claw modifie sa copie de
  travail, que le moteur pousse à la fin.
- **`claw-remote`** (les 28 outils de `claude-code-remote`, plus `jtest`) : vos autres sessions (lister,
  lire, leur écrire, en créer, les arrêter, archiver, renommer, étiqueter), les routines, le
  suivi d'une pull request, la surveillance d'une adresse, la liste de vos dépôts et la copie en
  lecture d'un autre dépôt à côté du travail, les machines, la documentation de Claw Web. Un
  message envoyé par une session à une autre y paraît sous « Message de la session #n de
  Claw » : la session qui le reçoit le prend pour ce qu'il est, un message d'un autre agent, pas
  vos mots.
- **`claw-docs`** (8 outils, ceux de `Claude_Docs`) : des documents que Claw écrit pour vous
  (menu, « Documents »).

Chaque appel paraît dans le journal du passage avec son titre en français (« GitHub : Lire une
issue #12 »). Un refus de GitHub dit l'autorisation qui manque au jeton (`CLAW_REPO_TOKEN`).

Avec Claw Code sur votre machine, les mêmes serveurs se lancent sur l'entrée standard, avec
votre jeton (`GITHUB_TOKEN`, `GH_TOKEN` ou celui de `gh auth token`) ; à mettre dans le
`.claw.json` du projet :

```json
{
  "mcpServers": {
    "github": { "command": "python3", "args": [".github/claw-web/mcp_server.py", "--stdio", "github"] },
    "claw-remote": { "command": "python3", "args": [".github/claw-web/mcp_server.py", "--stdio", "claw-remote"],
                     "env": { "CLAW_WEB_ENGINE": "abasse-ali/claw-sessions" } },
    "claw-docs": { "command": "python3", "args": [".github/claw-web/mcp_server.py", "--stdio", "claw-docs"],
                   "env": { "CLAW_WEB_ENGINE": "abasse-ali/claw-sessions" } }
  }
}
```

(chemin de `mcp_server.py` dans votre copie du code de Claw ; le dépôt est celui du dossier
courant, ou `--repo propriétaire/nom`). Sur votre machine, ce que vous demandez est fait : fusion
et création de dépôt comprises.

## Documents

Quand vous demandez un rapport, un plan ou des notes à garder, Claw écrit un document (menu,
« Documents ») : des onglets en Markdown, des sections qu'il remplit une à une (« En cours : … »
le temps de les écrire), des commentaires. Un commentaire s'ajoute sous le document ;
« Commenter et demander à Claw » l'envoie aussi à la session qui l'a écrit, qui répond dans le
document. Téléchargement en Markdown ou en page HTML depuis l'appli ; Claw sait aussi en faire un
PDF ou un document Word (`export`), rangé avec le document. Les documents sont sur la branche
`claw-web-fichiers` du dépôt des sessions, dans `documents/<identifiant>/` (`doc.json`, un
fichier `.md` par onglet, `exports/`). Sans document, la page le dit et propose « Demander à
Claw » : une demande de rapport toute prête attend dans la saisie de l'accueil, à envoyer telle
quelle ou à changer.

## Routines et suivis

Claw peut programmer un message vers une session (« chaque matin à 9 h, fais le point sur les
issues », « relance les tests dans deux heures »), créer une session neuve à chaque fois, suivre
une pull request (nouveaux commentaires, revues, vérifications finies, fusion : écrits dans la
session, qui repart) ou surveiller une adresse. Menu, « Routines » : la liste, l'interrupteur de
chacune, « Lancer maintenant » et « Supprimer » (deux appuis), et les suivis, avec « Arrêter ».
Sans routine, « Demander à Claw » met une demande d'exemple (un point chaque matin à 9 h) dans la
saisie de l'accueil.

Le programmateur est un workflow du dépôt des sessions qui passe toutes les heures (le plus
petit intervalle, comme dans Claude Code) : une routine part à l'heure près. Il faut l'ajouter
une fois au dépôt des sessions, `.github/workflows/claw-routines.yml` (la page « Routines » le
donne s'il manque) :

```yaml
on:
  schedule:
    - cron: "17 * * * *"
  workflow_dispatch:
permissions:
  actions: read
  contents: write
  issues: write
jobs:
  routines:
    uses: abasse-ali/claw_code/.github/workflows/claw-routines.yml@main
    with:
      runner: ${{ vars.CLAW_RUNNER }}
    secrets: inherit
```

Il prend une minute d'Actions par heure au plus ; aucune sur « Ma machine » (variable du dépôt
`CLAW_RUNNER` = `claw-pc`). Les routines et les suivis sont rangés sur `claw-web-fichiers`,
dans `routines/`.

## Notifications

Réglages, Notifications, « Activer les notifications » : chaque appareil abonné est prévenu quand
Claw lui pose une question, quand il a fini un passage, ou s'il s'est arrêté (pas pour un passage
que vous avez annulé), même l'appli fermée. Toucher la notification ouvre la session. Sur iPhone,
ajoutez d'abord Claw Web à l'écran d'accueil (Partager, « Sur l'écran d'accueil ») et ouvrez-le
depuis son icône : Safari n'envoie de notifications qu'aux applis installées.

Ce sont des notifications Web Push : la première activation crée la paire de clés VAPID du
moteur ; la clé privée part chiffrée dans le secret `CLAW_PUSH_VAPID_PRIVATE` du dépôt des
sessions (elle ne reste pas sur l'appareil), la clé publique et les appareils abonnés vont dans
ses variables `CLAW_PUSH_VAPID_PUBLIC` et `CLAW_PUSH_SUBSCRIPTIONS` (le jeton a besoin de
l'autorisation Variables). À la fin d'un passage, le moteur (`.github/claw-web/push.py`) chiffre
la notification pour chaque appareil (RFC 8291) et l'envoie ; un appareil désabonné ou effacé
est retiré de la liste. « Couper les notifications ici » ne coupe que cet appareil.

## JTest : essayer sur le téléphone

« JTest » (sous la conversation d'une session) lance le travail de la session sur votre serveur
gratuit d'Oracle et donne un lien https à ouvrir sur l'iPhone : un site, une appli web, ou un
programme. Claw le fait aussi de lui-même quand vous voulez essayer le résultat (« je veux le
tester sur mon iPhone ») : après sa réponse, le lien arrive dans la session.

- Le serveur clone la branche de la session, devine comment lancer le projet (Vite, Next.js,
  Astro, Nuxt, Angular, Create React App, `npm run dev` ou `npm start`, Flask, FastAPI, Django,
  Streamlit, site statique, dans la racine ou un dossier `app/`, `web/`, `frontend/`…), installe
  ses dépendances et le lance. Le lien vient d'un tunnel Cloudflare gratuit
  (`https://….trycloudflare.com`) : rien à ouvrir ni à régler chez Oracle.
- **Réservé à vous.** L'adresse du tunnel est publique, mais elle mène à une porte (sur le
  serveur) qui ne laisse passer que la clé de cette JTest : une clé aléatoire par lancement,
  écrite seulement dans la carte de la session, dans le dépôt privé des sessions. Le bouton
  « Ouvrir sur le téléphone » la porte ; la porte la change en cookie (24 heures au plus, la
  durée du test) et l'enlève de l'adresse. Sans elle, ou avec l'adresse seule, la page dit
  « Accès réservé ». La clé n'est jamais montrée à l'agent ni au fournisseur du modèle, et
  JTest refuse de démarrer si le dépôt des sessions est public.
- Un programme qui n'est pas un site (Python, Node, Rust…) s'ouvre dans un terminal web, derrière
  la même porte (son mot de passe, la porte le donne elle-même) ; le programme se relance
  d'Entrée quand il se termine.
- La carte JTest de la session : « Ouvrir sur le téléphone », l'heure jusqu'à laquelle il reste en
  ligne (4 heures par défaut, 24 au plus), « Arrêter » (deux appuis), « Relancer ». Un échec dit
  pourquoi, avec le journal de l'installation.
- Un projet particulier : un fichier `jtest.json` à la racine, par exemple
  `{"command": "npm run dev -- --port {port}", "install": ["npm ci"], "path": "/admin"}`.
- Au plus trois JTest en ligne à la fois sur le serveur (la plus ancienne s'arrête). Les
  programmes lancés n'ont ni jeton ni clé.

Il faut le serveur gratuit d'Oracle (« Sessions sur votre machine » ci-dessous) : JTest tourne
sur l'exécuteur `claw-pc` à processeur ARM, et le site y reste en ligne après le passage.

## Applis en React, bibliothèques prêtes

Une appli perso (budget, suivi, habitudes, collection) se construit en **React** : Claw part
d'un projet prêt, avec ses bibliothèques déjà installées (React, TypeScript, Vite, Tailwind
CSS, Framer Motion, des icônes, des graphiques, des menus et des curseurs accessibles), et
compose les écrans avec les composants de son kit d'appli au lieu de tout écrire à la main.
Chaque écran peut vivre dans son propre composant, avec son bouton + à lui (ouvrir la fenêtre
d'ajout de cet écran) : les onglets les suivent de la même façon.
Le journal le dit en une ligne : « Prépare le projet · appli React ». Avant de regarder l'appli,
Claw la construit dans le dossier `docs/`, celui que « Mettre en ligne » publie. Pour une page
sans React, demandez-la « en HTML simple ». Claw sait aussi poser un serveur Node (Express,
Socket.IO) ou Python (FastAPI) ; ceux-là demandent un hébergeur, GitHub Pages ne publiant que
des pages.

Le modèle choisi travaille seul : il ne demande plus d'avis à d'autres modèles, sauf pour la
conception du design, la note des écrans et la description des images.

## Fusionner, mettre en ligne

- « Fusionner » (seulement pour une ancienne session dont la pull request est restée ouverte,
  deux appuis) : la branche de la session rejoint la branche de départ.
- « Mettre en ligne » : publie un site statique sur GitHub Pages (dossier `docs/` ou racine),
  depuis la branche de la session telle quelle. Sur un compte
  GitHub gratuit, GitHub Pages ne publie que les dépôts publics : l'appli le signale et propose
  de rendre le dépôt public. Un site déjà en ligne a son bouton « Dépublier le site » (deux
  appuis) : il quitte GitHub Pages, son adresse ne répond plus, le code et les branches restent.
  Claw (l'agent) n'a aucun jeton GitHub : il agit sur GitHub par les
  outils du moteur (« Outils MCP »), et demande la fusion ou la mise en ligne de la session à
  `claw-github`, fait après la poussée.

## Arrêter, supprimer

- « Arrêter » (deux appuis : le premier arme le bouton 4 s, une barre se vide) annule un passage
  en cours : ce qui est fait reste sur la branche de la session ; un message de suivi reprend le
  travail. La session dit aussitôt « Arrêt en cours » (plus aucune étape ne paraît ensuite) et
  se relit toutes les 1,5 s : GitHub finit le passage en une dizaine de secondes, le temps que
  Claw et ses commandes s'arrêtent (celles lancées en arrière-plan comprises) et que le travail
  soit poussé. Un passage tout juste lancé, que GitHub ne liste pas encore, est arrêté dès qu'il
  paraît. Si GitHub tarde (machine injoignable), « Forcer l'arrêt » paraît au bout de 45 s : il
  coupe tout de suite, sans pousser le travail de ce passage.
- « Supprimer » dans une session, ou « Choisir » dans la liste des sessions (menu) pour en
  supprimer plusieurs : la session et ses messages sont effacés de GitHub, sans retour possible.
  En option, sa branche `claw/session-<n>` (une pull request ouverte est alors fermée) et ses
  passages dans GitHub Actions. Une session au travail s'arrête d'abord.
- Effacer une issue est réservé à l'administrateur du dépôt du moteur. Si GitHub le refuse au
  jeton, la session est seulement retirée de l'appli (fermée, sans l'étiquette `claw-web`) ;
  « Delete issue » sur GitHub l'efface.

Le premier passage compile Claw (quelques minutes) ; le binaire reste ensuite en cache et les
passages suivants démarrent en moins d'une minute.

## Sécurité

- **Réservée à son propriétaire.** La page est publique (GitHub Pages), mais l'appli ne sert
  qu'aux comptes GitHub de `OWNERS` (début de `js/app.js`) : le jeton d'un autre compte est
  refusé avant tout, sans code posé ni rien de gardé sur l'appareil, et un appareil où un autre
  compte s'était installé est vidé à l'ouverture. Sans jeton du propriétaire, aucune session,
  aucun dépôt, aucun secret n'est lisible : c'est GitHub qui les garde. Sur un appareil sans
  verrou, seul l'écran de connexion s'affiche (jeton GitHub du propriétaire) : aucune page, pas
  même une adresse comme `#/reglages`, ne se montre avant. Le code de la page reste public,
  comme tout site GitHub Pages ; il ne contient aucun secret.
- **Verrou de l'appli.** Dès qu'un jeton est gardé sur l'appareil, l'appli fait choisir un code
  (8 caractères au moins) et le demande à chaque ouverture et après 15 minutes en arrière-plan.
  Le jeton GitHub et la clé OpenRouter sont chiffrés par ce code (AES-GCM, clé tirée du code par
  PBKDF2-SHA-256, 600 000 tours) et ne sont jamais gardés en clair : quelqu'un qui prend le
  téléphone, ou lit le stockage du navigateur, n'a pas le jeton sans le code. Trois codes faux
  imposent une attente qui double (30 s, puis 1 min...). Réglages, groupe « Verrou » :
  verrouiller tout de suite, changer le code (l'actuel est demandé). Code oublié : « Effacer les
  clés de cet appareil » sur l'écran du verrou, puis recoller le jeton et choisir un nouveau code.
- **Face ID.** Proposé juste après le choix du code, ou dans Réglages, groupe « Verrou ». Une clé
  d'accès (passkey) de l'appareil, avec l'extension PRF de WebAuthn, libère après vérification
  du visage un secret qui déchiffre le code du verrou : à l'ouverture, Face ID est demandé tout
  de suite, et le code reste le secours. Il faut iOS 18 (Safari ou l'appli ajoutée à l'écran
  d'accueil) et le trousseau iCloud ; ailleurs, Touch ID ou Windows Hello font de même. Changer
  le code retire Face ID, à réactiver avec le nouveau.
- Le jeton n'est jamais réaffiché dans les réglages (champ vide : il reste) ; « Effacer le jeton
  de cet appareil » le retire.
- Les secrets partent chiffrés pour la clé publique du dépôt du moteur (sealed box de libsodium,
  avec [tweetnacl](https://github.com/dchest/tweetnacl-js) et BLAKE2b) ; la clé OpenRouter n'est
  pas gardée par l'appli.
- La page ne peut joindre que `api.github.com` et `openrouter.ai` (politique de sécurité du
  contenu) ; les messages sont affichés sans aucun HTML venu d'ailleurs.
- Dans le moteur, l'agent travaille sans aucun jeton GitHub dans son environnement : le dépôt
  est cloné puis son adresse remise sans jeton, et la poussée a lieu après l'agent.
- Les outils MCP du moteur tournent dans le processus du moteur, sur un port local réservé par un
  secret du passage. Le jeton du dépôt leur arrive dans un fichier du job, lu puis effacé avant
  que l'agent ne démarre : il n'est ni dans l'environnement de l'étape, ni sur le disque pendant
  le travail. Ils s'en tiennent au dépôt de la session.
