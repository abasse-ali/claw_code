#!/bin/bash
# Exécuteur de Claw Web sur un serveur Linux toujours allumé (le serveur gratuit d'Oracle Cloud, ou
# tout serveur Ubuntu 22.04/24.04, processeur ARM ou Intel/AMD) : les sessions y tournent jour et
# nuit, sans minutes de GitHub et sans votre PC. L'équivalent de claw-pc.ps1, pour Linux ; voir
# README.md. À lancer en root (sudo), ou tel quel comme script de démarrage du serveur (cloud-init).
#
#   claw-cloud.sh install  --repo propriétaire/claw-sessions --token JETON
#                          installe Docker, inscrit l'exécuteur auprès du dépôt des sessions (le
#                          jeton d'inscription, valable une heure, vient de l'appli : Réglages,
#                          Sessions, « Serveur dans le nuage »), construit l'image et la lance ;
#                          elle redémarre avec le serveur.
#   claw-cloud.sh register --repo propriétaire/claw-sessions --token JETON
#                          nouvelle inscription (jeton expiré pendant l'installation, ou exécuteur
#                          supprimé dans GitHub).
#   claw-cloud.sh update   reconstruit l'image et relance le conteneur, sans nouvelle inscription ;
#                          refusé pendant une session.
#   claw-cloud.sh status   état du conteneur et dernières lignes de son journal.
#   claw-cloud.sh stop | start
#   claw-cloud.sh remove   supprime le conteneur, l'image et le volume (l'exécuteur reste à
#                          retirer dans GitHub : Settings, Actions, Runners).
#
# Le jeton d'inscription n'est jamais écrit sur la ligne de commande de Docker ni dans un journal :
# il passe par un fichier effacé aussitôt.
set -euo pipefail

SOURCE="${CLAW_CLOUD_SOURCE:-https://raw.githubusercontent.com/abasse-ali/claw_code/main/.github/claw-web/pc}"
HOME_DIR=/opt/claw-cloud
IMAGE=claw-cloud-runner
BASE_IMAGE=claw-cloud-base
CONTAINER=claw-cloud
VOLUME=claw-cloud-runner
NAME="${CLAW_CLOUD_NAME:-claw-cloud}"
LABELS="${CLAW_CLOUD_LABELS:-claw-pc}"

say() { printf '[claw-cloud] %s\n' "$*"; }
fail() { say "ERREUR : $*" >&2; exit 1; }

usage() {
  cat <<'AIDE'
claw-cloud.sh install  --repo propriétaire/claw-sessions --token JETON   (installation complète)
claw-cloud.sh register --repo propriétaire/claw-sessions --token JETON   (nouvelle inscription)
claw-cloud.sh update | status | stop | start | remove
Le jeton d'inscription (une heure) vient de l'appli : Réglages, Sessions, « Serveur dans le nuage ».
AIDE
  exit "${1:-0}"
}

need_root() {
  [ "$(id -u)" -eq 0 ] || fail "à lancer en root : sudo $0 $*"
}

# Docker (paquet d'Ubuntu), lancé avec le serveur.
ensure_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    say "Installation de Docker..."
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io curl jq ca-certificates >/dev/null
  fi
  # BuildKit (paquet docker-buildx) : sans lui, l'ancien constructeur de Docker ne donne pas
  # l'architecture au Dockerfile (vu à l'essai sur Ubuntu 24.04) ; elle est aussi passée à la main.
  if ! docker buildx version >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker-buildx >/dev/null 2>&1 || true
  fi
  command -v jq >/dev/null 2>&1 || { apt-get update -qq; apt-get install -y -qq jq >/dev/null; }
  systemctl enable --now docker >/dev/null 2>&1 || true
  docker info >/dev/null 2>&1 || fail "Docker ne répond pas (systemctl status docker)."
}

# Fichiers de l'image (Dockerfile, démarrage), pris dans le code de Claw.
fetch_sources() {
  mkdir -p "$HOME_DIR"
  for file in Dockerfile entrypoint.sh; do
    curl -fsSL "$SOURCE/$file" -o "$HOME_DIR/$file" || fail "téléchargement de $file impossible ($SOURCE)."
  done
}

runner_version() {
  local tag
  tag="$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest | jq -r .tag_name)"
  [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]] || fail "version de l'exécuteur de GitHub introuvable ($tag)."
  printf '%s' "${BASH_REMATCH[1]}"
}

build() {
  local target="$1" tag="$2" version
  version="$(runner_version)"
  say "Construction de l'image $tag (exécuteur $version, étape $target)..."
  docker build --quiet --target "$target" --tag "$tag" --build-arg "RUNNER_VERSION=$version" \
    --build-arg "TARGETARCH=$(dpkg --print-architecture 2>/dev/null || echo amd64)" "$HOME_DIR" >/dev/null \
    || fail "la construction de l'image a échoué (docker build --target $target $HOME_DIR)."
}

container_state() {
  docker ps -a --filter "name=^${CONTAINER}$" --format '{{.State}}' 2>/dev/null || true
}

# Une session en cours : le processus Runner.Worker tourne dans le conteneur.
busy() {
  [ "$(container_state)" = "running" ] && docker exec "$CONTAINER" pgrep -f Runner.Worker >/dev/null 2>&1
}

# Inscription auprès de GitHub dans le volume, avec l'image légère (l'exécuteur seul) : faite dans
# les premières minutes, tant que le jeton est valable.
register() {
  local repo="$1" token="$2" envfile
  [[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail "dépôt invalide : $repo (propriétaire/nom)."
  [ -n "$token" ] || fail "jeton d'inscription manquant (--token)."
  build runner "$BASE_IMAGE"
  envfile="$(mktemp)"
  chmod 600 "$envfile"
  printf 'RUNNER_URL=https://github.com/%s\nRUNNER_TOKEN=%s\nRUNNER_NAME=%s\nRUNNER_LABELS=%s\nREGISTER_ONLY=1\n' \
    "$repo" "$token" "$NAME" "$LABELS" > "$envfile"
  say "Inscription de l'exécuteur $NAME auprès de $repo..."
  if ! docker run --rm --env-file "$envfile" --volume "$VOLUME:/home/runner/actions-runner" "$BASE_IMAGE"; then
    rm -f "$envfile"
    fail "inscription refusée par GitHub : jeton expiré (une heure) ? Demandez-en un nouveau dans l'appli, puis : sudo $0 register --repo $repo --token NOUVEAU_JETON"
  fi
  rm -f "$envfile"
}

start_container() {
  docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
  docker run --detach --name "$CONTAINER" --restart unless-stopped \
    --volume "$VOLUME:/home/runner/actions-runner" "$IMAGE" >/dev/null \
    || fail "le lancement du conteneur a échoué."
  sleep 5
  [ "$(container_state)" = "running" ] || fail "le conteneur s'est arrêté : docker logs $CONTAINER"
}

install() {
  need_root install
  local repo="$1" token="$2"
  ensure_docker
  fetch_sources
  if docker volume inspect "$VOLUME" >/dev/null 2>&1 \
     && docker run --rm --volume "$VOLUME:/data" --entrypoint test "$BASE_IMAGE" -f /data/.runner 2>/dev/null; then
    say "Exécuteur déjà inscrit : inscription gardée."
  else
    register "$repo" "$token"
  fi
  build full "$IMAGE"
  start_container
  say "Exécuteur $NAME en ligne (étiquette $LABELS). Dans l'appli : Réglages, Sessions, « Sur ma machine »."
}

update() {
  need_root update
  [ -n "$(container_state)" ] || fail "aucun exécuteur installé : sudo $0 install --repo ... --token ..."
  busy && fail "une session tourne sur ce serveur : recommencez quand elle est finie."
  ensure_docker
  fetch_sources
  build full "$IMAGE"
  # Arrêt propre d'abord : l'exécuteur ferme sa connexion à GitHub (sinon le nouveau conteneur
  # attend plusieurs minutes, « A session for this runner already exists »).
  docker stop --time 60 "$CONTAINER" >/dev/null 2>&1 || true
  start_container
  say "Exécuteur reconstruit et relancé."
}

status() {
  local state
  state="$(container_state)"
  if [ -z "$state" ]; then
    say "Aucun exécuteur installé."
    return
  fi
  say "Conteneur $CONTAINER : $state$(busy && printf ' (session en cours)' || true)"
  docker logs --tail 8 "$CONTAINER" 2>&1 | sed 's/^/  /'
}

remove() {
  need_root remove
  docker stop --time 60 "$CONTAINER" >/dev/null 2>&1 || true
  docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  docker image rm "$IMAGE" "$BASE_IMAGE" >/dev/null 2>&1 || true
  say "Exécuteur supprimé du serveur. Retirez-le aussi dans GitHub : Settings, Actions, Runners."
}

action="${1:-status}"
shift || true
repo=""
token=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) repo="${2:-}"; shift 2 ;;
    --token) token="${2:-}"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) fail "option inconnue : $1" ;;
  esac
done

case "$action" in
  install) install "$repo" "$token" ;;
  register)
    need_root register
    ensure_docker
    fetch_sources
    docker stop --time 60 "$CONTAINER" >/dev/null 2>&1 || true
    docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
    docker volume rm "$VOLUME" >/dev/null 2>&1 || true
    register "$repo" "$token"
    docker image inspect "$IMAGE" >/dev/null 2>&1 || build full "$IMAGE"
    start_container
    say "Exécuteur réinscrit et relancé."
    ;;
  update) update ;;
  status) status ;;
  stop) need_root stop; docker stop --time 60 "$CONTAINER" >/dev/null && say "Exécuteur arrêté." ;;
  start) need_root start; docker start "$CONTAINER" >/dev/null && say "Exécuteur relancé." ;;
  remove) remove ;;
  -h|--help|help) usage 0 ;;
  *) usage 1 ;;
esac
