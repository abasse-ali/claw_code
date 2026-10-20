#!/bin/bash
# Exécuteur de Claw Web sur un serveur Linux toujours allumé (le serveur gratuit d'Oracle Cloud, ou
# tout serveur Ubuntu, Debian, Oracle Linux, processeur ARM ou Intel/AMD) : les sessions y tournent
# jour et nuit, sans minutes de GitHub et sans votre PC. L'équivalent de claw-pc.ps1, pour Linux ;
# voir README.md. À lancer en root (sudo), ou tel quel comme script de démarrage du serveur
# (cloud-init).
#
#   claw-cloud.sh install  --repo propriétaire/claw-sessions --token JETON [--shared]
#                          installe Docker, inscrit l'exécuteur auprès du dépôt des sessions (le
#                          jeton d'inscription, valable une heure, vient de l'appli : Réglages,
#                          Sessions, « Serveur dans le nuage »), construit l'image et la lance ;
#                          elle redémarre avec le serveur. --shared : serveur qui fait déjà tourner
#                          autre chose ; Claw y prend au plus la moitié du processeur et de la
#                          mémoire, avec une priorité basse (--cpus N et --memory 6g pour régler).
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
# Limites du conteneur (serveur partagé), gardées pour les mises à jour.
LIMITS_FILE="$HOME_DIR/limits"
IMAGE=claw-cloud-runner
BASE_IMAGE=claw-cloud-base
CONTAINER=claw-cloud
VOLUME=claw-cloud-runner
NAME="${CLAW_CLOUD_NAME:-claw-cloud}"
LABELS="${CLAW_CLOUD_LABELS:-claw-pc}"
# Place libre conseillée pour l'image (Linux, Python, Rust, Node, navigateur) et les sessions.
DISK_NEEDED_MB=10000

say() { printf '[claw-cloud] %s\n' "$*"; }
fail() { say "ERREUR : $*" >&2; exit 1; }

usage() {
  cat <<'AIDE'
claw-cloud.sh install  --repo propriétaire/claw-sessions --token JETON [--shared]
                       (installation complète ; --shared sur un serveur qui fait déjà tourner
                       autre chose : Claw y prend au plus la moitié du processeur et de la mémoire)
claw-cloud.sh register --repo propriétaire/claw-sessions --token JETON   (nouvelle inscription)
claw-cloud.sh update | status | stop | start | remove
Le jeton d'inscription (une heure) vient de l'appli : Réglages, Sessions, « Serveur dans le nuage ».
AIDE
  exit "${1:-0}"
}

need_root() {
  [ "$(id -u)" -eq 0 ] || fail "à lancer en root : sudo $0 $*"
}

# Architecture au sens de Docker (amd64, arm64), lue dans le noyau : vaut pour toute distribution.
docker_arch() {
  case "$(uname -m)" in
    aarch64 | arm64) echo arm64 ;;
    *) echo amd64 ;;
  esac
}

# Docker, lancé avec le serveur : paquets d'Ubuntu ou de Debian, ou Docker CE sur Oracle Linux
# (dont la commande `docker` n'est souvent que Podman, qui ne garde pas un conteneur en vie
# d'un redémarrage à l'autre).
ensure_docker() {
  if command -v docker >/dev/null 2>&1 && docker --version 2>/dev/null | grep -qi podman; then
    fail "la commande docker de ce serveur est Podman : installez Docker (Docker CE), puis recommencez."
  fi
  if ! command -v docker >/dev/null 2>&1; then
    say "Installation de Docker..."
    if command -v apt-get >/dev/null 2>&1; then
      apt-get update -qq
      DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io curl ca-certificates >/dev/null
    elif command -v dnf >/dev/null 2>&1; then
      dnf install -y -q dnf-plugins-core curl >/dev/null
      dnf config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo >/dev/null
      dnf install -y -q docker-ce docker-ce-cli containerd.io docker-buildx-plugin >/dev/null
    else
      fail "ni apt-get ni dnf : installez Docker à la main, puis recommencez."
    fi
  fi
  # BuildKit (docker-buildx) : sans lui, l'ancien constructeur de Docker ne donne pas
  # l'architecture au Dockerfile (vu à l'essai sur Ubuntu 24.04) ; elle est aussi passée à la main.
  if ! docker buildx version >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker-buildx >/dev/null 2>&1 || true
  fi
  systemctl enable --now docker >/dev/null 2>&1 || true
  docker info >/dev/null 2>&1 || fail "Docker ne répond pas (systemctl status docker)."
}

# Place libre là où Docker range ses images.
check_disk() {
  local root free
  root="$(docker info --format '{{.DockerRootDir}}' 2>/dev/null || echo /var/lib/docker)"
  [ -d "$root" ] || root=/
  free="$(df -Pm "$root" | awk 'NR == 2 {print $4}')"
  if [ -n "$free" ] && [ "$free" -lt "$DISK_NEEDED_MB" ]; then
    say "Attention : ${free} Mo libres pour Docker, environ ${DISK_NEEDED_MB} Mo conseillés."
  fi
}

# Serveur partagé : la moitié des processeurs et de la mémoire au plus, priorité basse.
shared_limits() {
  local cpus memory_mb
  cpus="$(nproc)"
  memory_mb="$(awk '/^MemTotal:/ {print int($2 / 1024)}' /proc/meminfo)"
  awk -v c="$cpus" -v m="$memory_mb" 'BEGIN {
    half = c / 2; if (half < 1) half = 1
    mem = int(m / 2); if (mem < 2048) mem = 2048
    printf "--cpus=%s --memory=%dm --cpu-shares=256", half, mem
  }'
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
  tag="$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest \
    | grep -o '"tag_name": *"v[0-9.]*"' | grep -o 'v[0-9.]*' | head -n 1)"
  [[ "$tag" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]] || fail "version de l'exécuteur de GitHub introuvable ($tag)."
  printf '%s' "${BASH_REMATCH[1]}"
}

build() {
  local target="$1" tag="$2" version
  version="$(runner_version)"
  say "Construction de l'image $tag (exécuteur $version, $(docker_arch), étape $target)..."
  docker build --quiet --target "$target" --tag "$tag" --build-arg "RUNNER_VERSION=$version" \
    --build-arg "TARGETARCH=$(docker_arch)" "$HOME_DIR" >/dev/null \
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
  local limits=()
  if [ -s "$LIMITS_FILE" ]; then
    read -r -a limits < "$LIMITS_FILE"
    say "Limites du conteneur : ${limits[*]}"
  fi
  docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
  docker run --detach --name "$CONTAINER" --restart unless-stopped "${limits[@]}" \
    --volume "$VOLUME:/home/runner/actions-runner" "$IMAGE" >/dev/null \
    || fail "le lancement du conteneur a échoué."
  sleep 5
  [ "$(container_state)" = "running" ] || fail "le conteneur s'est arrêté : docker logs $CONTAINER"
}

install() {
  need_root install
  local repo="$1" token="$2"
  ensure_docker
  check_disk
  fetch_sources
  if [ -n "$container_limits" ]; then
    printf '%s\n' "$container_limits" > "$LIMITS_FILE"
  fi
  if docker volume inspect "$VOLUME" >/dev/null 2>&1 \
     && docker run --rm --volume "$VOLUME:/data" --entrypoint test "$BASE_IMAGE" -f /data/.runner 2>/dev/null; then
    say "Exécuteur déjà inscrit : inscription gardée."
  else
    register "$repo" "$token"
  fi
  build full "$IMAGE"
  start_container
  say "Exécuteur $NAME en ligne (étiquette $LABELS). Dans l'appli : Réglages, Sessions, « Ma machine »."
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
  [ -s "$LIMITS_FILE" ] && say "Limites : $(cat "$LIMITS_FILE")"
  docker logs --tail 8 "$CONTAINER" 2>&1 | sed 's/^/  /'
}

remove() {
  need_root remove
  docker stop --time 60 "$CONTAINER" >/dev/null 2>&1 || true
  docker rm --force "$CONTAINER" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  docker image rm "$IMAGE" "$BASE_IMAGE" >/dev/null 2>&1 || true
  rm -f "$LIMITS_FILE"
  say "Exécuteur supprimé du serveur. Retirez-le aussi dans GitHub : Settings, Actions, Runners."
}

action="${1:-status}"
shift || true
repo=""
token=""
container_limits=""
cpus=""
memory=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repo) repo="${2:-}"; shift 2 ;;
    --token) token="${2:-}"; shift 2 ;;
    --shared) container_limits="$(shared_limits)"; shift ;;
    --cpus) cpus="${2:-}"; shift 2 ;;
    --memory) memory="${2:-}"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) fail "option inconnue : $1" ;;
  esac
done
if [ -n "$cpus$memory" ]; then
  [[ -z "$cpus" || "$cpus" =~ ^[0-9]+(\.[0-9]+)?$ ]] || fail "--cpus attend un nombre (1, 1.5)."
  [[ -z "$memory" || "$memory" =~ ^[0-9]+[mMgG]$ ]] || fail "--memory attend une taille (6g, 4096m)."
  container_limits="${cpus:+--cpus=$cpus} ${memory:+--memory=$memory} --cpu-shares=256"
  container_limits="${container_limits# }"
fi

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
