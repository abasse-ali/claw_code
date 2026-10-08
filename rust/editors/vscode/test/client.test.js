// Test du client sans VS Code : démarre `claw serve`, envoie une sélection,
// vérifie que le modèle (faux serveur) la reçoit et que la page accepte d'être
// affichée dans un panneau VS Code.
// Usage : node test/client.test.js <claw> <dossier> <journal du faux modèle>
"use strict";

const assert = require("assert");
const fs = require("fs");
const http = require("http");
const { ClawServer, selectionMessage } = require("../claw-client");

const [claw, cwd, logPath] = process.argv.slice(2);
const results = [];

function check(label, condition, detail) {
  results.push(Boolean(condition));
  console.log(`${condition ? "OK  " : "ÉCHEC "}${label}${condition || detail === undefined ? "" : `  (${detail})`}`);
}

function get(port, path) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port, path, headers: { Host: `127.0.0.1:${port}` } }, (response) => {
        let body = "";
        response.on("data", (chunk) => (body += chunk));
        response.on("end", () => resolve({ headers: response.headers, body }));
      })
      .on("error", reject);
  });
}

(async () => {
  fs.writeFileSync(logPath, "");
  const server = new ClawServer({ clawPath: claw, cwd, model: "ollama:t", permissionMode: "read-only" });
  const started = await server.start();
  check("claw serve démarré, adresse et jeton lus", started.port > 0 && /^[A-Za-z0-9_-]{20,}$/.test(started.token), JSON.stringify(started));

  const page = await get(started.port, "/");
  const csp = page.headers["content-security-policy"] || "";
  check("page affichable dans un panneau VS Code (--embed)", csp.includes("frame-ancestors 'self' vscode-webview:") && !page.headers["x-frame-options"], csp);

  const message = selectionMessage({
    instruction: "Explique ce code",
    relativePath: "src/lib.rs",
    startLine: 3,
    endLine: 5,
    languageId: "rust",
    text: "fn main() {\n    println!(\"ok\");\n}",
  });
  assert.ok(message.includes("```rust"));
  await server.send(message);
  let received = "";
  for (let attempt = 0; attempt < 60 && !received.includes("Explique ce code"); attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    received = fs.readFileSync(logPath, "utf8");
  }
  check("sélection reçue par le modèle (fichier, lignes, code)", received.includes("Explique ce code") && received.includes("src/lib.rs") && received.includes("lignes 3-5"), received.slice(0, 200));

  let rejected = false;
  try {
    await new ClawServer({ clawPath: claw, cwd }).request("POST", "/api/message", { text: "x" });
  } catch (_) {
    rejected = true;
  }
  check("requête sans serveur démarré refusée proprement", rejected);

  server.stop();
  await new Promise((resolve) => setTimeout(resolve, 500));
  check("arrêt du serveur", !server.running);
  console.log(`\n${results.filter(Boolean).length}/${results.length} vérifications réussies`);
  process.exit(results.every(Boolean) ? 0 : 1);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
