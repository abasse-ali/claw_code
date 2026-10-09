// Pilotage de `claw serve` depuis Node : démarrage dans un dossier, lecture de
// l'adresse (avec son jeton), envoi de messages, arrêt. Sans dépendance à
// VS Code pour pouvoir être testé seul.
"use strict";

const { spawn } = require("child_process");
const fs = require("fs");
const http = require("http");
const net = require("net");
const path = require("path");

/**
 * Chemin complet de l'exécutable `claw` : le réglage « claw.path » s'il
 * désigne un fichier, sinon le premier `claw` du PATH. `null` s'il est
 * introuvable.
 */
function resolveExecutable(clawPath, env = process.env) {
  const name = clawPath || "claw";
  const windows = process.platform === "win32";
  const suffixes = windows ? ["", ...(env.PATHEXT || ".EXE;.CMD;.BAT").split(";").filter(Boolean)] : [""];
  const candidates = /[\\/]/.test(name) || path.isAbsolute(name)
    ? [path.resolve(name)]
    : (env.PATH || env.Path || "").split(path.delimiter).filter(Boolean).map((directory) => path.join(directory, name));
  for (const candidate of candidates) {
    for (const suffix of suffixes) {
      try {
        // Chemin réel : casse du disque, noms longs, cible d'un lien.
        if (fs.statSync(candidate + suffix).isFile()) return fs.realpathSync.native(candidate + suffix);
      } catch (_) {
        /* candidat suivant */
      }
    }
  }
  return null;
}

/** Empreinte d'un fichier (date et taille) : change quand claw est réinstallé. */
function fileStamp(file) {
  try {
    const stat = fs.statSync(file);
    return `${Math.round(stat.mtimeMs)}-${stat.size}`;
  } catch (_) {
    return null;
  }
}

/**
 * Copie de travail de l'exécutable dans `directory`. Sous Windows, un
 * programme en cours d'exécution verrouille son fichier : lancer `claw.exe`
 * lui-même ferait échouer sa réinstallation tant que le panneau est ouvert.
 * Les anciennes copies sont retirées quand plus rien ne s'en sert.
 */
function workingCopy(executable, directory) {
  fs.mkdirSync(directory, { recursive: true });
  const target = path.join(directory, `claw-${fileStamp(executable)}${path.extname(executable)}`);
  if (!fs.existsSync(target)) fs.copyFileSync(executable, target);
  for (const name of fs.readdirSync(directory)) {
    const file = path.join(directory, name);
    if (file === target || !name.startsWith("claw-")) continue;
    try {
      fs.unlinkSync(file);
    } catch (_) {
      /* encore utilisée par une autre fenêtre */
    }
  }
  return target;
}

/**
 * Surveille l'exécutable installé ; `onChange` est appelé quand il a été
 * remplacé et que son écriture est finie (deux lectures identiques de suite).
 * Renvoie la fonction qui arrête la surveillance.
 */
function watchExecutable(executable, onChange, intervalMs = 2000) {
  let known = fileStamp(executable);
  let candidate = null;
  const timer = setInterval(() => {
    const now = fileStamp(executable);
    if (now === null || now === known) {
      candidate = null;
    } else if (candidate === now) {
      known = now;
      candidate = null;
      onChange();
    } else {
      candidate = now;
    }
  }, intervalMs);
  if (timer.unref) timer.unref();
  return () => clearInterval(timer);
}

/** Port TCP libre sur la boucle locale. */
function findFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

class ClawServer {
  /**
   * @param {{clawPath?: string, cwd: string, model?: string, permissionMode?: string, env?: object, startTimeoutMs?: number}} options
   */
  constructor(options) {
    this.options = options;
    this.process = null;
    this.url = null;
    this.port = null;
    this.token = null;
    this.stderr = "";
    this.exitListeners = [];
  }

  get running() {
    return this.process !== null && this.process.exitCode === null;
  }

  /** Démarre le serveur ; résout avec `{ url, port, token }`. */
  async start() {
    if (this.running && this.url) {
      return { url: this.url, port: this.port, token: this.token };
    }
    const port = await findFreePort();
    const args = [];
    if (this.options.model) args.push("--model", this.options.model);
    if (this.options.permissionMode) args.push("--permission-mode", this.options.permissionMode);
    args.push("serve", "--no-open", "--embed", "--port", String(port));
    const child = spawn(this.options.clawPath || "claw", args, {
      cwd: this.options.cwd,
      env: { ...process.env, ...(this.options.env || {}) },
      windowsHide: true,
    });
    this.process = child;
    this.stderr = "";
    child.stderr.on("data", (chunk) => {
      this.stderr = (this.stderr + chunk.toString("utf8")).slice(-8000);
    });
    child.on("exit", (code) => {
      this.process = null;
      this.url = null;
      for (const listener of this.exitListeners) listener(code, this.stderr);
    });
    return new Promise((resolve, reject) => {
      let buffer = "";
      const timer = setTimeout(() => {
        reject(new Error(`claw serve n'a pas démarré en ${(this.options.startTimeoutMs || 60000) / 1000} s\n${this.stderr}`));
      }, this.options.startTimeoutMs || 60000);
      child.on("error", (error) => {
        clearTimeout(timer);
        reject(new Error(`lancement de claw impossible (${error.message}) : réglez « claw.path »`));
      });
      child.on("exit", (code) => {
        clearTimeout(timer);
        reject(new Error(`claw serve s'est arrêté (code ${code})\n${this.stderr}`));
      });
      child.stdout.on("data", (chunk) => {
        buffer += chunk.toString("utf8");
        const match = buffer.match(/(http:\/\/[^\s]+\?token=([A-Za-z0-9_-]+))/);
        if (match && !this.url) {
          clearTimeout(timer);
          this.url = match[1];
          this.token = match[2];
          this.port = port;
          resolve({ url: this.url, port, token: this.token });
        }
      });
    });
  }

  onExit(listener) {
    this.exitListeners.push(listener);
  }

  /** Requête vers l'API de `claw serve`. */
  request(method, path, body) {
    if (!this.port || !this.token) {
      return Promise.reject(new Error("claw serve n'est pas démarré"));
    }
    const payload = body === undefined ? null : Buffer.from(JSON.stringify(body), "utf8");
    return new Promise((resolve, reject) => {
      const request = http.request(
        {
          host: "127.0.0.1",
          port: this.port,
          method,
          path,
          headers: {
            Host: `127.0.0.1:${this.port}`,
            "X-Claw-Token": this.token,
            ...(payload ? { "Content-Type": "application/json", "Content-Length": payload.length } : {}),
          },
        },
        (response) => {
          let data = "";
          response.on("data", (chunk) => (data += chunk));
          response.on("end", () => {
            if (response.statusCode >= 400) {
              let message = data;
              try {
                message = JSON.parse(data).error || data;
              } catch (_) {
                /* texte brut */
              }
              reject(new Error(`${response.statusCode} : ${message}`));
            } else {
              resolve(data ? safeJson(data) : null);
            }
          });
        },
      );
      request.on("error", reject);
      if (payload) request.write(payload);
      request.end();
    });
  }

  /**
   * S'abonne aux événements de `claw serve` (flux SSE, rôle `status` : ne
   * compte pas comme une page ouverte). Reconnexion automatique tant que le
   * serveur tourne ; `stop()` ferme l'abonnement. Avec `options.editor` (nom
   * de l'éditeur), l'abonné se déclare prêt à répondre aux demandes de
   * diagnostics de Claw (événements `editor_sync`).
   */
  subscribe(onEvent, options = {}) {
    const editor = options.editor ? `&editor=${encodeURIComponent(options.editor)}` : "";
    let retrying = false;
    const retry = () => {
      if (retrying || !this.running) return;
      retrying = true;
      setTimeout(() => {
        retrying = false;
        connect();
      }, 2000);
    };
    const connect = () => {
      if (!this.running || !this.port || !this.token) return;
      const request = http.get(
        {
          host: "127.0.0.1",
          port: this.port,
          path: `/api/events?role=status${editor}`,
          headers: { Host: `127.0.0.1:${this.port}`, "X-Claw-Token": this.token, Accept: "text/event-stream" },
        },
        (response) => {
          if (response.statusCode !== 200) {
            response.resume();
            retry();
            return;
          }
          let buffer = "";
          response.setEncoding("utf8");
          response.on("data", (chunk) => {
            buffer += chunk.replace(/\r\n/g, "\n");
            let end;
            while ((end = buffer.indexOf("\n\n")) >= 0) {
              const block = buffer.slice(0, end);
              buffer = buffer.slice(end + 2);
              const name = (block.match(/^event:\s*(.*)$/m) || [])[1];
              const data = block
                .split("\n")
                .filter((line) => line.startsWith("data:"))
                .map((line) => line.slice(5).replace(/^ /, ""))
                .join("\n");
              if ((name === "event" || name === "snapshot") && data) {
                try {
                  const parsed = JSON.parse(data);
                  // L'état initial arrive dans un « snapshot » : rendu comme un événement d'état.
                  onEvent(name === "snapshot" ? { type: "status", status: parsed.status } : parsed);
                } catch (_) {
                  /* événement illisible ignoré */
                }
              }
            }
          });
          response.on("end", retry);
          response.on("error", retry);
        },
      );
      request.on("error", retry);
      this.eventsRequest = request;
    };
    connect();
  }

  /** Envoie un message à l'agent (mis en file d'attente si un tour est en cours). */
  send(text, images) {
    return this.request("POST", "/api/message", { text, images: images || [] });
  }

  interrupt() {
    return this.request("POST", "/api/interrupt", {});
  }

  stop() {
    if (this.eventsRequest) {
      this.eventsRequest.destroy();
      this.eventsRequest = null;
    }
    if (this.process) {
      this.process.kill();
      this.process = null;
    }
    this.url = null;
  }
}

function safeJson(text) {
  try {
    return JSON.parse(text);
  } catch (_) {
    return text;
  }
}

/** Message décrivant une sélection de code (fichier, lignes, langage). */
function selectionMessage({ instruction, relativePath, startLine, endLine, languageId, text }) {
  const header = instruction && instruction.trim() ? `${instruction.trim()}\n\n` : "";
  const lines = startLine === endLine ? `ligne ${startLine}` : `lignes ${startLine}-${endLine}`;
  return `${header}Extrait de \`${relativePath}\` (${lines}) :\n\`\`\`${languageId || ""}\n${text}\n\`\`\``;
}

/** Gravités de VS Code (0 erreur, 1 avertissement, 2 information, 3 indice) dans le vocabulaire de Claw. */
const SEVERITIES = ["error", "warning", "info", "hint"];

/** Un diagnostic de VS Code tel que Claw l'attend (ligne et colonne à partir de 1). */
function clawDiagnostic(diagnostic) {
  const start = (diagnostic.range && diagnostic.range.start) || { line: 0, character: 0 };
  const code = diagnostic.code && typeof diagnostic.code === "object" ? diagnostic.code.value : diagnostic.code;
  return {
    line: start.line + 1,
    column: start.character + 1,
    severity: SEVERITIES[diagnostic.severity] || "info",
    message: String(diagnostic.message || ""),
    source: diagnostic.source || null,
    code: code === undefined || code === null || code === "" ? null : String(code),
  };
}

/** Chemin montré à l'agent : relatif au dossier de travail quand le fichier s'y trouve. */
function workspacePath(cwd, file) {
  const relative = path.relative(cwd, file);
  const inside = relative && !relative.startsWith("..") && !path.isAbsolute(relative);
  return inside ? relative.split(path.sep).join("/") : file;
}

/**
 * Réponse à une demande de diagnostics : `entries` est une liste de
 * `[chemin absolu, diagnostics de VS Code]`.
 */
function diagnosticsAnswer(cwd, entries) {
  return entries.map(([file, diagnostics]) => ({
    path: workspacePath(cwd, file),
    diagnostics: (diagnostics || []).map(clawDiagnostic),
  }));
}

module.exports = {
  ClawServer,
  findFreePort,
  selectionMessage,
  resolveExecutable,
  fileStamp,
  workingCopy,
  watchExecutable,
  clawDiagnostic,
  workspacePath,
  diagnosticsAnswer,
};
