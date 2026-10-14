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
- **D'un passage à l'autre** : le clone est neuf à chaque passage et `.claw/` n'est jamais
  poussé. La demande donne d'abord les mots de l'utilisateur (la tâche, ou les nouveaux
  messages), puis la conversation précédente et le dépôt dans des notes `[Claw]` : Claw ne
  décide le mode projet que sur ce qui précède la première ligne `[Claw]`. L'état du mode
  projet de la session (`.claw/project.json` : actif ou arrêté, plan relu, démonstration
  réussie) est gardé dans le cache d'Actions (`claw-web-state-<session>-...`). Sans cet état
  (nouvelle session sur un projet déjà fusionné), Claw reprend le mode projet de lui-même quand
  `SPEC.md` et un `PLAN.md` à jalons sont à la racine.
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
| W1-1 | Moteur : workflow, préparation (session, dépôt, branche), agent avec progression, publication ; essai sur GitHub avec le faux modèle | fait le 14 octobre 2026 (6744ce0, 2ed50f0, fc2d0bb) |
| W1-2 | Appli : réglages (jeton, clé, secrets), accueil (dépôt, branche, modèle, tâche), sessions, conversation en direct, appli installable (manifeste, service worker, icônes) | fait le 14 octobre 2026 (dépôt `claw-web`, 8c0b5f2) |
| W1-3 | Publication : dépôt public `claw-web`, GitHub Pages, essai réel de bout en bout | en cours : Pages actif (https://abasse-ali.github.io/claw-web/) ; secrets `OPENROUTER_API_KEY` et `CLAW_REPO_TOKEN` posés (vérifications du moteur au vert dans l'appli) ; vrai modèle gratuit essayé (session 2) ; reste le premier essai de l'utilisateur sur un de ses dépôts |
| W1-4 | Retours de l'utilisateur : pas de zoom, comme une appli ; réglages sur leur propre page (`#/reglages`), plus à l'accueil | fait le 14 octobre 2026 (`claw-web` 1e04535) |
| W1-5 | Création de dépôt depuis le choix du dépôt (nom, description, public ou privé, `.gitignore`, premier commit) ; mode projet continu d'un passage à l'autre du moteur | fait le 14 octobre 2026 (`claw-web` 6e72294 ; moteur et Claw dans ce dépôt) |

## Essais

- Moteur avec le faux modèle (session 1, sur `claw_code` lui-même) : premier passage 4 min 26 s
  compilation comprise, progression mise à jour en direct, réponse publiée ; la publication
  échouait sur `git add -A -- . ':(exclude).claw'` (git refuse de nommer un chemin ignoré) :
  `.claw/` étant exclu par `.git/info/exclude`, un simple `git add -A` suffit. Message de suivi
  sur la même session : binaire en cache, fini en 3 s, modifications non publiées faute de
  `CLAW_REPO_TOKEN` (dit dans la réponse). Les caches des tests (`__pycache__`...) sont
  maintenant exclus du commit.
- Appli au format téléphone (Edge sans fenêtre piloté par le protocole DevTools, émulation
  390 x 844 tactile ; `--headless=new` impose sinon une largeur minimale et coupe la capture) :
  premier lancement, accueil connecté (sessions récentes, puces dépôt, branche, modèle),
  session (messages, progression, réponses, actions). Chiffrement des secrets : BLAKE2b
  identique à Python sur 33 cas, aller-retour de la sealed box. Rendu Markdown : HTML et liens
  `javascript:` neutralisés.
- Continuité du mode projet (6c62306, e48d59d), sur GitHub avec le faux modèle, session 4
  (tâche en mode projet, sans consigne d'écriture) : premier passage en 21 s (binaire en
  cache), mode projet activé, état mis en cache ; message de suivi : « État du mode projet
  repris du passage précédent », mode projet poursuivi ; aucun fichier modifié, rien publié.
  Un passage au faux modèle ne reçoit plus aucune clé réelle (relecteurs compris).
- Incident pendant cet essai : relancée avec le faux modèle, la session 1 portait dans sa tâche
  une consigne d'écriture (`[mock:write ...]`) ; `CLAW_REPO_TOKEN` étant maintenant posé, le
  moteur a poussé `claw/session-1` et ouvert la pull request n° 3 dans `claw_code` (le fichier
  du faux modèle et quatre fichiers `.port_sessions/` laissés par les essais Python du dépôt).
  Pull request fermée, branche supprimée, sa CI arrêtée. Les essais du moteur se font
  désormais sur une session dont la tâche ne fait rien écrire.

