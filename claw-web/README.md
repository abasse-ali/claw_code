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
     **All repositories** et **Administration** en lecture et écriture. Modifier ces réglages
     garde la valeur du jeton : rien à refaire dans l'appli.
2. Ouvrez l'appli, collez le jeton, enregistrez.
3. Dans « Moteur » : collez votre clé OpenRouter (ou laissez le champ vide si elle est déjà
   gardée sur l'appareil), laissez coché « Donner aussi ce jeton au moteur », puis
   « Configurer le moteur ». L'appli pose deux secrets dans le dépôt du moteur,
   chiffrés dans le navigateur : `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN`.
4. Choisissez un dépôt et une branche, ou créez un dépôt neuf, puis décrivez la tâche. Dans un
   dépôt neuf, une demande de création (« Crée une application... ») fait passer Claw en mode
   projet : cahier des charges, plan en jalons vérifiés, démonstration finale. Les messages
   de suivi le poursuivent (une nouvelle session aussi, quand `SPEC.md` et `PLAN.md` sont dans
   le dépôt) ; « sans mode projet » l'arrête pour la session.

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
