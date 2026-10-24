// Outils MCP du moteur, pour la page « Outils MCP » de l'appli. Produit par
// `python3 .github/claw-web/mcp_server.py --catalog > claw-web/js/mcp-tools.js` : ne pas modifier.
export default {
 "version": 1,
 "servers": [
  {
   "server": "github",
   "title": "GitHub",
   "about": "Issues, pull requests, revues, branches, fichiers, Actions, versions et recherche, sur le dépôt de la session. Équivalent du serveur github de Claude Code.",
   "tools": [
    {
     "name": "actions_get",
     "title": "Lire une ressource GitHub Actions",
     "read_only": true
    },
    {
     "name": "actions_list",
     "title": "Lister GitHub Actions",
     "read_only": true
    },
    {
     "name": "actions_run_trigger",
     "title": "Lancer ou relancer un workflow",
     "read_only": false
    },
    {
     "name": "add_comment_to_pending_review",
     "title": "Commenter une revue en attente",
     "read_only": false
    },
    {
     "name": "add_issue_comment",
     "title": "Commenter une issue ou une pull request",
     "read_only": false
    },
    {
     "name": "add_reply_to_pull_request_comment",
     "title": "Répondre à un commentaire de revue",
     "read_only": false
    },
    {
     "name": "create_branch",
     "title": "Créer une branche",
     "read_only": false,
     "note": "Pas la branche de la session : Claw la crée en poussant votre travail"
    },
    {
     "name": "create_or_update_file",
     "title": "Écrire un fichier sur GitHub",
     "read_only": false,
     "note": "Pas sur la branche de la session ni sur la base : vos changements passent par la copie de travail"
    },
    {
     "name": "create_pull_request",
     "title": "Créer une pull request",
     "read_only": false
    },
    {
     "name": "create_repository",
     "title": "Créer un dépôt",
     "read_only": false,
     "note": "Sur Claw Web, c'est vous qui créez le dépôt (« Créer un dépôt »)"
    },
    {
     "name": "delete_file",
     "title": "Supprimer un fichier sur GitHub",
     "read_only": false,
     "note": "Pas sur la branche de la session ni sur la base : vos changements passent par la copie de travail"
    },
    {
     "name": "disable_pr_auto_merge",
     "title": "Désactiver la fusion automatique",
     "read_only": false
    },
    {
     "name": "enable_pr_auto_merge",
     "title": "Activer la fusion automatique",
     "read_only": false,
     "note": "Pull request de la session seulement, quand vous demandez sa fusion"
    },
    {
     "name": "fork_repository",
     "title": "Forker un dépôt",
     "read_only": false,
     "note": "Sur Claw Web, les outils restent sur le dépôt de la session"
    },
    {
     "name": "get_check_run",
     "title": "Lire une vérification",
     "read_only": true
    },
    {
     "name": "get_commit",
     "title": "Lire un commit",
     "read_only": true
    },
    {
     "name": "get_file_contents",
     "title": "Lire un fichier ou un dossier",
     "read_only": true
    },
    {
     "name": "get_job_logs",
     "title": "Lire les journaux d'un job",
     "read_only": true
    },
    {
     "name": "get_label",
     "title": "Lire une étiquette",
     "read_only": true
    },
    {
     "name": "get_latest_release",
     "title": "Lire la dernière version publiée",
     "read_only": true
    },
    {
     "name": "get_me",
     "title": "Mon profil GitHub",
     "read_only": true
    },
    {
     "name": "get_release_by_tag",
     "title": "Lire une version publiée par son tag",
     "read_only": true
    },
    {
     "name": "get_tag",
     "title": "Lire un tag",
     "read_only": true
    },
    {
     "name": "get_team_members",
     "title": "Membres d'une équipe",
     "read_only": true
    },
    {
     "name": "get_teams",
     "title": "Équipes",
     "read_only": true
    },
    {
     "name": "issue_read",
     "title": "Lire une issue",
     "read_only": true
    },
    {
     "name": "issue_write",
     "title": "Créer ou modifier une issue",
     "read_only": false
    },
    {
     "name": "list_branches",
     "title": "Lister les branches",
     "read_only": true
    },
    {
     "name": "list_commits",
     "title": "Lister les commits",
     "read_only": true
    },
    {
     "name": "list_issue_fields",
     "title": "Lister les champs d'issue",
     "read_only": true
    },
    {
     "name": "list_issue_types",
     "title": "Lister les types d'issue",
     "read_only": true
    },
    {
     "name": "list_issues",
     "title": "Lister les issues",
     "read_only": true
    },
    {
     "name": "list_pull_requests",
     "title": "Lister les pull requests",
     "read_only": true
    },
    {
     "name": "list_releases",
     "title": "Lister les versions publiées",
     "read_only": true
    },
    {
     "name": "list_repository_collaborators",
     "title": "Lister les collaborateurs",
     "read_only": true
    },
    {
     "name": "list_tags",
     "title": "Lister les tags",
     "read_only": true
    },
    {
     "name": "merge_pull_request",
     "title": "Fusionner une pull request",
     "read_only": false,
     "note": "Pull request de la session seulement, fusionnée par Claw après la poussée, quand vous le demandez"
    },
    {
     "name": "pull_request_read",
     "title": "Lire une pull request",
     "read_only": true
    },
    {
     "name": "pull_request_review_write",
     "title": "Écrire une revue de pull request",
     "read_only": false
    },
    {
     "name": "push_files",
     "title": "Pousser des fichiers sur GitHub",
     "read_only": false,
     "note": "Pas sur la branche de la session ni sur la base : vos changements passent par la copie de travail"
    },
    {
     "name": "request_copilot_review",
     "title": "Demander une revue à Copilot",
     "read_only": false
    },
    {
     "name": "resolve_review_thread",
     "title": "Résoudre une discussion de revue",
     "read_only": false
    },
    {
     "name": "run_secret_scanning",
     "title": "Chercher des secrets",
     "read_only": true,
     "note": "Recherche faite par le moteur, avec des motifs de secrets connus"
    },
    {
     "name": "search_code",
     "title": "Chercher dans le code",
     "read_only": true,
     "note": "Recherche limitée au dépôt de la session"
    },
    {
     "name": "search_commits",
     "title": "Chercher des commits",
     "read_only": true,
     "note": "Recherche limitée au dépôt de la session"
    },
    {
     "name": "search_issues",
     "title": "Chercher des issues",
     "read_only": true,
     "note": "Recherche limitée au dépôt de la session"
    },
    {
     "name": "search_pull_requests",
     "title": "Chercher des pull requests",
     "read_only": true,
     "note": "Recherche limitée au dépôt de la session"
    },
    {
     "name": "search_repositories",
     "title": "Chercher des dépôts",
     "read_only": true
    },
    {
     "name": "search_users",
     "title": "Chercher des utilisateurs",
     "read_only": true
    },
    {
     "name": "sub_issue_write",
     "title": "Modifier les sous-issues",
     "read_only": false
    },
    {
     "name": "unresolve_review_thread",
     "title": "Rouvrir une discussion de revue",
     "read_only": false
    },
    {
     "name": "update_issue_comment",
     "title": "Modifier un commentaire",
     "read_only": false
    },
    {
     "name": "update_pull_request",
     "title": "Modifier une pull request",
     "read_only": false
    },
    {
     "name": "update_pull_request_branch",
     "title": "Mettre à jour la branche d'une pull request",
     "read_only": false,
     "note": "Pas pour la pull request de la session : la base se fusionne dans la copie de travail, avec git"
    }
   ]
  },
  {
   "server": "claw-remote",
   "title": "Sessions et routines",
   "about": "Sessions de Claw Web, messages entre sessions, routines programmées, suivi des pull requests, dépôts. Équivalent de claude-code-remote.",
   "tools": [
    {
     "name": "add_repo",
     "title": "Ajouter un dépôt",
     "read_only": false,
     "note": "En lecture seule, cloné à côté du dépôt de la session"
    },
    {
     "name": "archive_session",
     "title": "Archiver une session",
     "read_only": false
    },
    {
     "name": "confirm_repository_trust",
     "title": "Confirmer la confiance",
     "read_only": true,
     "note": "Rien à confirmer : Claw Web ne travaille que sur vos dépôts"
    },
    {
     "name": "create_session",
     "title": "Créer une session",
     "read_only": false
    },
    {
     "name": "create_trigger",
     "title": "Créer une routine",
     "read_only": false,
     "note": "Au plus une fois par heure ; déclenchée par le workflow claw-routines.yml"
    },
    {
     "name": "delete_trigger",
     "title": "Supprimer une routine",
     "read_only": false
    },
    {
     "name": "fire_trigger",
     "title": "Lancer une routine",
     "read_only": false
    },
    {
     "name": "get_event",
     "title": "Lire un événement",
     "read_only": true
    },
    {
     "name": "get_session",
     "title": "Lire une session",
     "read_only": true
    },
    {
     "name": "get_trigger",
     "title": "Lire une routine",
     "read_only": true
    },
    {
     "name": "interrupt_session",
     "title": "Arrêter une session",
     "read_only": false
    },
    {
     "name": "list_environments",
     "title": "Lister les machines",
     "read_only": true
    },
    {
     "name": "list_events",
     "title": "Lister les événements",
     "read_only": true
    },
    {
     "name": "list_repos",
     "title": "Lister les dépôts",
     "read_only": true
    },
    {
     "name": "list_sessions",
     "title": "Lister les sessions",
     "read_only": true
    },
    {
     "name": "list_triggers",
     "title": "Lister les routines",
     "read_only": true
    },
    {
     "name": "read_documentation",
     "title": "Lire la documentation",
     "read_only": true
    },
    {
     "name": "register_repo_root",
     "title": "Lire les consignes d'un dépôt",
     "read_only": true
    },
    {
     "name": "send_later",
     "title": "Programmer un message",
     "read_only": false,
     "note": "À l'heure près (le programmateur passe toutes les heures)"
    },
    {
     "name": "send_message",
     "title": "Écrire à une session",
     "read_only": false
    },
    {
     "name": "set_session_tags",
     "title": "Étiqueter des sessions",
     "read_only": false
    },
    {
     "name": "set_session_title",
     "title": "Renommer une session",
     "read_only": false
    },
    {
     "name": "subscribe_pr_activity",
     "title": "Suivre une pull request",
     "read_only": false,
     "note": "Dépôt de la session ; nouveautés relevées toutes les heures"
    },
    {
     "name": "unarchive_session",
     "title": "Désarchiver une session",
     "read_only": false
    },
    {
     "name": "unsubscribe_pr_activity",
     "title": "Ne plus suivre une pull request",
     "read_only": false
    },
    {
     "name": "unwatch_url",
     "title": "Ne plus surveiller une adresse",
     "read_only": false
    },
    {
     "name": "update_trigger",
     "title": "Modifier une routine",
     "read_only": false
    },
    {
     "name": "watch_url",
     "title": "Surveiller une adresse",
     "read_only": false,
     "note": "Adresse relue toutes les heures (pas de webhook entrant)"
    }
   ]
  },
  {
   "server": "claw-docs",
   "title": "Documents",
   "about": "Documents à onglets et commentaires, rangés dans le dépôt des sessions. Équivalent de Claude_Docs.",
   "tools": [
    {
     "name": "guide",
     "title": "Guide des documents",
     "read_only": true
    },
    {
     "name": "batch",
     "title": "Créer ou modifier un document",
     "read_only": false,
     "note": "Opérations faites en mémoire puis enregistrées sur la branche des fichiers, doc.json d'abord ; GitHub n'a pas de transaction sur plusieurs fichiers."
    },
    {
     "name": "create",
     "title": "Ajouter à un document",
     "read_only": false,
     "note": "Pas de graphiques, schémas ni pièces jointes ; un blob est un export rangé avec le document."
    },
    {
     "name": "read",
     "title": "Lire un document",
     "read_only": true
    },
    {
     "name": "update",
     "title": "Modifier un document",
     "read_only": false,
     "note": "Contenu en Markdown : réécriture, ajout, remplacement exact de mots, ou section en attente remplie."
    },
    {
     "name": "delete",
     "title": "Supprimer d'un document",
     "read_only": false
    },
    {
     "name": "export",
     "title": "Exporter un onglet",
     "read_only": true,
     "note": "PDF et DOCX simples faits par le moteur (titres, listes, tableaux, code, liens) ; pas d'images."
    },
    {
     "name": "query",
     "title": "Lire les commentaires",
     "read_only": true,
     "note": "Sans document : la liste des documents du dépôt des sessions."
    }
   ]
  }
 ]
};
