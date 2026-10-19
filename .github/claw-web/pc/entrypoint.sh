#!/bin/sh
# Démarrage de l'exécuteur de Claw Web sur le PC. Au premier démarrage, il s'inscrit auprès du
# dépôt des sessions avec le jeton d'inscription (une heure) que donne claw-pc.ps1 ; ses
# identifiants restent ensuite dans le volume, et un redémarrage de Docker le relance seul.
set -eu
cd /home/runner/actions-runner
if [ ! -f .runner ]; then
  : "${RUNNER_URL:?RUNNER_URL manquant (adresse du dépôt des sessions)}"
  : "${RUNNER_TOKEN:?RUNNER_TOKEN manquant (jeton d'inscription, claw-pc.ps1 install)}"
  ./config.sh --unattended --replace --url "$RUNNER_URL" --token "$RUNNER_TOKEN" \
    --name "${RUNNER_NAME:-claw-pc}" --labels claw-pc --work _work
fi
unset RUNNER_TOKEN
exec ./run.sh
