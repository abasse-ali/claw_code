# Exécuteur GitHub Actions de Claw Web sur votre PC (Docker Desktop) : les sessions tournent ici,
# sans minutes de GitHub. Une machine Linux qui a ce que le moteur trouve sur les machines
# ubuntu-latest de GitHub : Python, git, Rust, Node, Chrome (captures de l'outil Screenshot).
# Construite et lancée par claw-pc.ps1 ; voir README.md.
FROM ubuntu:24.04

# Versions données par claw-pc.ps1 (la dernière version de l'exécuteur de GitHub).
ARG RUNNER_VERSION
ARG NODE_VERSION=24.12.0

# pytest installé, et `pip install` permis sans --break-system-packages : vu dans une session,
# l'agent perdait deux requêtes à installer pytest, refusé par Ubuntu (PEP 668) la première fois.
ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    PIP_BREAK_SYSTEM_PACKAGES=1

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates curl git jq python3 python3-pip python3-venv python3-pytest build-essential pkg-config \
       libssl-dev unzip xz-utils zstd tini fonts-liberation fonts-dejavu-core fonts-noto-core \
    && curl -fsSL -o /tmp/chrome.deb https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb \
    && apt-get install -y --no-install-recommends /tmp/chrome.deb \
    && rm -f /tmp/chrome.deb \
    && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
       | tar -xJ -C /usr/local --strip-components=1 \
    && rm -rf /var/lib/apt/lists/*

# L'exécuteur de GitHub et ses dépendances (.NET), dans le dossier que garde le volume.
RUN test -n "$RUNNER_VERSION" \
    && useradd -m -u 1001 -s /bin/bash runner \
    && mkdir -p /home/runner/actions-runner \
    && cd /home/runner/actions-runner \
    && curl -fsSL -o runner.tar.gz \
       "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
    && tar xzf runner.tar.gz \
    && rm runner.tar.gz \
    && ./bin/installdependencies.sh \
    && rm -rf /var/lib/apt/lists/* \
    && chown -R runner:runner /home/runner

# Chrome sans son bac à sable interne : Docker interdit les espaces de noms dont il a besoin
# (« Failed to move to new namespace: Operation not permitted »). Le conteneur, sans privilège et
# sous le filtre seccomp de Docker, isole déjà Chrome et Claw du PC ; Claw, qui y lance toute
# commande, n'a rien de plus à craindre de Chrome. Claw cherche `google-chrome` dans le PATH :
# /usr/local/bin passe avant /usr/bin.
RUN for name in google-chrome google-chrome-stable; do \
      printf '#!/bin/sh\nexec /opt/google/chrome/google-chrome --no-sandbox "$@"\n' > "/usr/local/bin/$name"; \
      chmod 755 "/usr/local/bin/$name"; \
    done

# Jamais root : Chrome refuse de tourner en root sans --no-sandbox, que Claw n'ajoute pas.
USER runner
WORKDIR /home/runner/actions-runner
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/home/runner/.cargo/bin:$PATH

COPY --chown=runner:runner entrypoint.sh /home/runner/entrypoint.sh
ENTRYPOINT ["tini", "--", "/bin/sh", "/home/runner/entrypoint.sh"]
