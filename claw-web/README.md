# Claw web

Appli web de [Claw](https://github.com/abasse-ali/claw_code) (dossier `claw-web` du dépôt du
code de Claw), l'agent de code qui travaille avec
des modèles gratuits : choisissez un dépôt GitHub, une branche et un modèle, décrivez une tâche ;
Claw y travaille dans GitHub Actions, pousse une branche, ouvre la pull request et vous répond.
Installable sur le téléphone (« Ajouter à l'écran d'accueil »).

Adresse : https://abasse-ali.github.io/claw_code/ (publiée par le workflow `pages.yml` à chaque
changement de ce dossier ; l'ancienne adresse, https://abasse-ali.github.io/claw-web/, y renvoie).

## Comment ça marche

- Cette appli est une page statique, sans serveur : elle parle directement à l'API de GitHub
  (et à la liste publique des modèles d'OpenRouter), avec votre jeton.
- Le travail se fait dans le **dépôt du moteur**, un dépôt privé (par défaut
  `abasse-ali/claw-sessions`) : son workflow `claw-web.yml` appelle le moteur, workflow du dépôt
  public du code de Claw, qui lance Claw sur le dépôt de la tâche, sur la branche de la session,
  puis pousse et ouvre la pull request. Les journaux et les minutes de GitHub Actions restent
  dans le dépôt privé.
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
- **NVIDIA** : environ 40 requêtes par minute (« 40/min »), sans quota par jour.

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

## Fusionner, mettre en ligne

- « Fusionner » (à côté de la pull request ouverte, deux appuis) : la branche de la session
  rejoint la branche de départ.
- « Mettre en ligne » : publie un site statique sur GitHub Pages (dossier `docs/` ou racine),
  après fusion de la pull request ou depuis la branche de la session telle quelle. Sur un compte
  GitHub gratuit, GitHub Pages ne publie que les dépôts publics : l'appli le signale et propose
  de rendre le dépôt public. Claw (l'agent) n'a aucun accès à GitHub : il renvoie à ces boutons.

## Arrêter, supprimer

- « Arrêter » (deux appuis) annule un passage en cours : ce qui est fait reste sur la branche
  de la session, sans pull request ; un message de suivi reprend le travail.
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

- Le jeton reste dans le stockage de ce navigateur (bouton « Effacer le jeton de cet appareil »).
- Les secrets partent chiffrés pour la clé publique du dépôt du moteur (sealed box de libsodium,
  avec [tweetnacl](https://github.com/dchest/tweetnacl-js) et BLAKE2b) ; la clé OpenRouter n'est
  pas gardée par l'appli.
- La page ne peut joindre que `api.github.com` et `openrouter.ai` (politique de sécurité du
  contenu) ; les messages sont affichés sans aucun HTML venu d'ailleurs.
- Dans le moteur, l'agent travaille sans aucun jeton GitHub dans son environnement : le dépôt
  est cloné puis son adresse remise sans jeton, et la poussée a lieu après l'agent.
