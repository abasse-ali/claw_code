// Pilotage de `claw serve` depuis Node : démarrage dans un dossier, lecture de
// l'adresse (avec son jeton), envoi de messages, arrêt. Sans dépendance à
// VS Code pour pouvoir être testé seul.
"use strict";

const { spawn } = require("child_process");
const http = require("http");
const net = require("net");

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

  /** Envoie un message à l'agent (mis en file d'attente si un tour est en cours). */
  send(text, images) {
    return this.request("POST", "/api/message", { text, images: images || [] });
  }

  interrupt() {
    return this.request("POST", "/api/interrupt", {});
  }

  stop() {
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

module.exports = { ClawServer, findFreePort, selectionMessage };
