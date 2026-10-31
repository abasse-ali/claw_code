// Aides au raisonnement de Claw (rust/crates/runtime/src/harness.rs) que l'appli laisse choisir,
// page « Aides de Claw » : clé (sa variable est CLAW_ suivi de la clé en majuscules), nom, ce
// qu'elle fait. Même liste, dans le même ordre, que AIDS de .github/claw-web/common.py (un essai
// du moteur le vérifie). Le mode projet, le mode équipe et Ultracode sont les options de la
// saisie ; les erreurs de l'éditeur (VS Code) et le modèle de planification (mode plan)
// n'agissent pas dans un passage de Claw Web.

export const AID_GROUPS = [
  {
    title: "Vérifier le travail",
    items: [
      ["verify", "Vérification imposée", "Quand le modèle dit avoir fini, Claw lance la vérification du projet (compilation, tests) et lui renvoie les erreurs."],
      ["diagnostics", "Syntaxe immédiate", "Après chaque modification, la syntaxe du fichier est contrôlée et l'erreur jointe au résultat."],
      ["pitfalls", "Pièges du code", "Un fichier de code tout juste écrit est relu pour les pièges qui passent la syntaxe (un nom jamais défini…)."],
      ["run_scripts", "Script jamais lancé", "Un script écrit et jamais lancé : le modèle doit l'essayer avant de répondre."],
      ["entry_point", "Script muet", "Un script Python qui définit main() sans l'appeler : Claw le dit au modèle."],
      ["checklist", "Contrôle des exigences", "Avant de conclure, le modèle confronte son travail à chaque partie de la demande, preuve à l'appui."],
      ["review", "Relecture à froid", "Avant de vous rendre la main, le modèle relit ses modifications dans une conversation neuve, sans le chemin qui l'y a mené."],
      ["test_guard", "Test réécrit", "Un test réécrit après un échec est signalé : faire passer un test n'est pas corriger."],
      ["design_review", "Revue visuelle", "Interface modifiée : Claw prend des captures et les fait noter par un modèle qui voit."],
      ["tried_commands", "Commandes essayées", "Une réponse conseille des commandes jamais lancées : le modèle est renvoyé les essayer."],
      ["visual_check", "Vérification visuelle", "Après chaque modification de l'interface, Claw ouvre la page sur un téléphone, la mesure et dit au modèle ce qui est cassé ; une page dite « vérifiée » est mesurée avant la réponse."],
    ],
  },
  {
    title: "Guider le modèle",
    items: [
      ["code_guide", "Méthode de code", "La méthode d'un ingénieur soigneux et les règles du langage, jointes à une demande de code."],
      ["design_guide", "Méthode de design", "La méthode d'un studio de design, jointe à une demande d'interface web."],
      ["edge_cases", "Cas limites", "Ce qu'un ingénieur vérifie d'office pour le genre de fonctionnalité demandé (dates, recherche, CSV…)."],
      ["problem_guide", "Signalement d'un problème", "Vous signalez un problème : une méthode de débogage est jointe, et des preuves demandées."],
      ["announce", "Annonce du plan", "Le modèle dit ce qu'il va faire avant ses premiers outils, puis une phrase à chaque étape."],
      ["work_questions", "Question sur le travail", "« Tu fais quoi ? » reçoit une réponse, sans relancer de travail."],
    ],
  },
  {
    title: "Le remettre sur les rails",
    items: [
      ["stall_guard", "Détection de blocage", "Appels répétés sans effet, échecs en série : Claw recadre le modèle."],
      ["persistence", "Persévérance", "Le modèle veut s'arrêter sur des échecs : il est renvoyé chercher une autre voie."],
      ["open_tasks", "Tâches finies", "Réponse finale avec des tâches pas faites : le modèle est renvoyé les finir."],
      ["announced_work", "Travail annoncé", "« Maintenant je construis… » sans rien avoir écrit : le modèle est renvoyé le faire."],
      ["empty_titles", "Titres sans contenu", "Réponse arrêtée sur des titres vides : le travail annoncé est à faire."],
      ["cut_replies", "Réponse coupée", "Réponse coupée par la limite de sortie du modèle : la suite est demandée."],
      ["kept_answer", "Compte rendu gardé", "Après un contrôle, un simple accusé de réception ne remplace pas le compte rendu."],
      ["questions", "Questions reposées", "Les questions écrites en fin de réponse deviennent des choix à toucher."],
      ["batch_calls", "Essais regroupés", "Une seule lecture ou commande par réponse, trois fois de suite : le modèle est invité à regrouper."],
      ["repeated_reads", "Relecture inutile", "Un fichier relu sans avoir changé : une seule copie gardée, et dit au modèle."],
    ],
  },
  {
    title: "Protéger les fichiers",
    items: [
      ["placeholder_guard", "Contenu abrégé", "« … reste du code inchangé … » est refusé avant d'écraser un fichier."],
      ["data_guard", "Données réécrites", "Des lignes d'un fichier de données réécrites : c'est le code qui les lit qu'il faut corriger."],
      ["lost_edits", "Modifications perdues", "Un fichier modifié puis effacé par une commande : sa dernière version est gardée."],
      ["path_help", "Chemins", "Un chemin inventé est retrouvé dans le projet ; sinon l'erreur montre les fichiers au nom proche."],
    ],
  },
  {
    title: "Contexte et outils",
    items: [
      ["repo_map", "Carte du projet", "Les fichiers et les principaux symboles du projet, donnés au début."],
      ["small_project", "Petit projet", "Un petit projet est montré en entier au premier tour, sans lectures une à une."],
      ["file_view", "Fichier montré après modification", "Le résultat d'une modification montre le fichier à jour : pas besoin de le relire."],
      ["error_sites", "Lignes des erreurs", "Une commande ou une vérification échoue : le code des lignes citées par l'erreur est joint au résultat."],
      ["lean_results", "Contexte allégé", "Les résultats gardés ne répètent pas au modèle ce qu'il vient d'écrire."],
      ["tool_diet", "Outils utiles seulement", "Les outils rarement utiles ne sont proposés que lorsqu'ils servent."],
      ["reasoning_passback", "Raisonnement conservé", "Le raisonnement du modèle est renvoyé à l'étape suivante au lieu d'être perdu."],
      ["lessons", "Leçons", "Le modèle note des leçons sur le projet, relues à la session suivante."],
      ["photos", "Photos libres", "L'outil Photo : photos sous licence libre enregistrées dans le projet avec leur crédit."],
    ],
  },
  {
    title: "Appeler d'autres modèles",
    items: [
      ["council", "Conseil et second avis", "Au deuxième échec, un autre modèle donne son diagnostic ; l'outil Consult demande d'autres avis."],
      ["escalation", "Escalade", "Le modèle n'avance plus : le modèle gratuit le plus puissant reprend la fin du tour."],
    ],
  },
];

export const AID_KEYS = AID_GROUPS.flatMap((group) => group.items.map(([key]) => key));
