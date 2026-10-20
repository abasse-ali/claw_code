#!/bin/sh
# Démarrage de l'exécuteur de Claw Web (PC ou serveur). Au premier démarrage, il s'inscrit auprès
# du dépôt des sessions avec le jeton d'inscription (une heure) que donne claw-pc.ps1 ou
# claw-cloud.sh ; ses identifiants restent ensuite dans le volume, et un redémarrage de Docker le
# relance seul. L'étiquette claw-pc est celle que choisit l'appli (« Sur ma machine ») : le PC et
# le serveur la portent tous deux, et une session va à celui qui est libre.
set -eu
cd /home/runner/actions-runner
if [ ! -f .runner ]; then
  : "${RUNNER_URL:?RUNNER_URL manquant (adresse du dépôt des sessions)}"
  : "${RUNNER_TOKEN:?RUNNER_TOKEN manquant (jeton d’inscription, claw-pc.ps1 install ou claw-cloud.sh install)}"
  ./config.sh --unattended --replace --url "$RUNNER_URL" --token "$RUNNER_TOKEN" \
    --name "${RUNNER_NAME:-claw-pc}" --labels "${RUNNER_LABELS:-claw-pc}" --work _work
fi
unset RUNNER_TOKEN
# Inscription seule (claw-cloud.sh, avant la construction de l'image complète) : rien à lancer.
if [ "${REGISTER_ONLY:-}" = "1" ]; then
  exit 0
fi
exec ./run.sh
