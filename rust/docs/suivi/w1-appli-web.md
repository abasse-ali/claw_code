# W1. Appli web de Claw sur GitHub Pages (comme Claude Code sur le web)

Demande de l'utilisateur, 14 octobre 2026 : « Je veux un github page de claw qui travaille sur
un repository comme avec claude et dispo sur mobile comme appli web » (capture de Claude Code
sur le web : dépôt, branche, modèle, « Décrivez une tâche ou posez une question »).

Choix de l'utilisateur : appli dans un **nouveau dépôt public `claw-web`** (aucun secret ni
code de Claw dedans, `claw_code` reste privé) ; l'agent travaille dans **GitHub Actions** ;
connexion par **jeton personnel** gardé dans le navigateur.

## Architecture

- **Appli** (`abasse-ali/claw-web`, GitHub Pages, installable sur le téléphone) : pages
  statiques, sans serveur. Choix du dépôt, de la branche et du modèle gratuit ; tâche ; liste
  des sessions ; conversation ; suivi en direct ; lien vers la pull request.
- **Moteur** (`claw_code`, privé) : workflow `.github/workflows/claw-web.yml`, lancé par
  l'appli (`workflow_dispatch` : dépôt, branche de départ, session, modèle). Il a le code de
  Claw et garde le binaire compilé en cache (recompilé seulement quand le code Rust change),
  clone le dépôt de la tâche avec le secret `CLAW_REPO_TOKEN`, travaille sur la branche
  `claw/session-<n>`, pousse, ouvre ou met à jour la pull request, et répond dans la session.
  Rien à installer dans les dépôts de l'utilisateur ; publics ou privés.
- **Sessions** : des issues privées de `claw_code` (étiquette `claw-web`). Le corps porte la
  tâche et ses réglages (dépôt, branche, modèle) ; chaque message de suivi est un commentaire ;
  le moteur publie un commentaire de progression mis à jour en direct, puis sa réponse.
- **Secrets du moteur** : `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN`, posés par l'appli
  (chiffrés dans le navigateur avec la clé publique du dépôt) ou à la main.
- **Sécurité** : le jeton n'est jamais dans l'environnement de l'agent (clone puis adresse du
  dépôt sans jeton ; poussée après la fin de l'agent) ; les commentaires de progression passent
  par le jeton du job, limité à `claw_code` et périmé à la fin du job. L'appli ne parle qu'à
  `api.github.com` et `openrouter.ai` (politique de sécurité du contenu), et affiche le Markdown
  en échappant tout le HTML.

## Étapes

| Étape | Contenu | État |
|---|---|---|
| W1-1 | Moteur : workflow, préparation (session, dépôt, branche), agent avec progression, publication ; essai sur GitHub avec le faux modèle | en cours |
| W1-2 | Appli : réglages (jeton, clé, secrets), accueil (dépôt, branche, modèle, tâche), sessions, conversation en direct, appli installable (manifeste, service worker, icônes) | à faire |
| W1-3 | Publication : dépôt public `claw-web`, GitHub Pages, essai réel de bout en bout | à faire |
