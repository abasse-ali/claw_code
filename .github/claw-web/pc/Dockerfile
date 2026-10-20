# Exécuteur GitHub Actions de Claw Web, sur votre PC (Docker Desktop, claw-pc.ps1) ou sur un
# serveur Linux toujours allumé, comme le serveur gratuit d'Oracle (claw-cloud.sh) : les sessions
# tournent ici, sans minutes de GitHub. Une machine Linux qui a ce que le moteur trouve sur les
# machines ubuntu-latest de GitHub : Python, git, Rust, Node, un navigateur (captures de l'outil
# Screenshot). Processeur Intel/AMD (amd64) ou ARM (arm64, le serveur gratuit d'Oracle).
#
# Deux étapes : `runner`, l'exécuteur seul (quelques minutes), sert à l'inscription auprès de
# GitHub tant que le jeton d'inscription (une heure) est valable ; l'image complète suit. Voir
# README.md.

FROM ubuntu:24.04 AS runner

# Version donnée par claw-pc.ps1 / claw-cloud.sh (la dernière version de l'exécuteur de GitHub) ;
# TARGETARCH (amd64 ou arm64) est donné par Docker.
ARG RUNNER_VERSION
ARG TARGETARCH

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git jq tini \
    && rm -rf /var/lib/apt/lists/*

# L'exécuteur de GitHub et ses dépendances (.NET), dans le dossier que garde le volume.
RUN test -n "$RUNNER_VERSION" \
    && case "$TARGETARCH" in arm64) arch=arm64 ;; *) arch=x64 ;; esac \
    && useradd -m -u 1001 -s /bin/bash runner \
    && mkdir -p /home/runner/actions-runner \
    && cd /home/runner/actions-runner \
    && curl -fsSL -o runner.tar.gz \
       "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-${arch}-${RUNNER_VERSION}.tar.gz" \
    && tar xzf runner.tar.gz \
    && rm runner.tar.gz \
    && ./bin/installdependencies.sh \
    && rm -rf /var/lib/apt/lists/* \
    && chown -R runner:runner /home/runner

COPY --chown=runner:runner entrypoint.sh /home/runner/entrypoint.sh
WORKDIR /home/runner/actions-runner
USER runner
ENTRYPOINT ["tini", "--", "/bin/sh", "/home/runner/entrypoint.sh"]


FROM runner AS full

ARG TARGETARCH
ARG NODE_VERSION=24.12.0
# Navigateur des processeurs ARM : Google Chrome n'existe pas pour Linux ARM, et le Chromium
# d'Ubuntu ne s'installe que par snap ; Playwright en fournit un, avec ses bibliothèques.
ARG PLAYWRIGHT_VERSION=1.62.1

USER root

# pytest installé, et `pip install` permis sans --break-system-packages : vu dans une session,
# l'agent perdait deux requêtes à installer pytest, refusé par Ubuntu (PEP 668) la première fois.
ENV PIP_BREAK_SYSTEM_PACKAGES=1 \
    PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       python3 python3-pip python3-venv python3-pytest build-essential pkg-config \
       libssl-dev unzip xz-utils zstd fonts-liberation fonts-dejavu-core fonts-noto-core \
    && case "$TARGETARCH" in arm64) node=arm64 ;; *) node=x64 ;; esac \
    && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node}.tar.xz" \
       | tar -xJ -C /usr/local --strip-components=1 \
    && rm -rf /var/lib/apt/lists/*

# Navigateur sans son bac à sable interne : Docker interdit les espaces de noms dont il a besoin
# (« Failed to move to new namespace: Operation not permitted »). Le conteneur, sans privilège et
# sous le filtre seccomp de Docker, isole déjà le navigateur et Claw de la machine ; Claw, qui y
# lance toute commande, n'a rien de plus à craindre du navigateur. Claw cherche `google-chrome`
# dans le PATH : /usr/local/bin passe avant /usr/bin.
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      npx -y "playwright@${PLAYWRIGHT_VERSION}" install --with-deps chromium \
      && browser="$(find "$PLAYWRIGHT_BROWSERS_PATH" -type f -name chrome -path '*chrome-linux*' | head -n 1)" \
      && test -x "$browser" \
      && rm -rf /var/lib/apt/lists/* /root/.npm; \
    else \
      curl -fsSL -o /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
      && apt-get update \
      && apt-get install -y --no-install-recommends /tmp/chrome.deb \
      && rm -f /tmp/chrome.deb \
      && rm -rf /var/lib/apt/lists/* \
      && browser=/opt/google/chrome/google-chrome; \
    fi \
    && for name in google-chrome google-chrome-stable; do \
         printf '#!/bin/sh\nexec %s --no-sandbox "$@"\n' "$browser" > "/usr/local/bin/$name"; \
         chmod 755 "/usr/local/bin/$name"; \
       done

# Jamais root : Chrome refuse de tourner en root sans --no-sandbox, que Claw n'ajoute pas.
USER runner
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/home/runner/.cargo/bin:$PATH
