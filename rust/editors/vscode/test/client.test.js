// Test du client sans VS Code : démarre `claw serve` face au faux modèle,
// envoie une sélection, vérifie que le modèle la reçoit et que la page accepte
// d'être affichée dans un panneau VS Code.
// Usage : node test/client.test.js <chemin de claw>
"use strict";

const assert = require("assert");
const fs = require("fs");
const http = require("http");
const os = require("os");
const path = require("path");
const { ClawServer, selectionMessage, resolveExecutable, workingCopy, watchExecutable } = require("../claw-client");
const { startMockModel } = require("./mock");

const results = [];

function check(label, condition, detail) {
  results.push(Boolean(condition));
  console.log(`${condition ? "OK     " : "ÉCHEC  "}${label}${condition || detail === undefined ? "" : `  (${detail})`}`);
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
  if (!process.argv[2]) throw new Error("usage : node test/client.test.js <chemin de claw>");
  const claw = path.resolve(process.argv[2]);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "claw-client-"));
  const cwd = path.join(root, "projet");
  fs.mkdirSync(cwd);
  const logPath = path.join(root, "modele.jsonl");
  fs.writeFileSync(logPath, "");
  const mock = await startMockModel(logPath);
  const env = { OLLAMA_BASE_URL: mock.baseUrl, CLAW_CONFIG_HOME: path.join(root, "claw-home"), CLAW_NO_CLAUDE_CONFIG: "1" };

  const server = new ClawServer({ clawPath: claw, cwd, model: "ollama:mock", permissionMode: "read-only", env });
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

  // Mise à jour de claw : exécutable retrouvé, copie de travail, surveillance.
  const installDir = path.join(root, "installe");
  fs.mkdirSync(installDir);
  const installed = path.join(installDir, path.basename(claw));
  fs.copyFileSync(claw, installed);
  const bare = path.basename(claw, path.extname(claw));
  const found = resolveExecutable(bare, { PATH: installDir, PATHEXT: process.env.PATHEXT });
  const real = fs.realpathSync.native(installed);
  check("exécutable retrouvé dans le PATH et par son chemin", found === real && resolveExecutable(installed) === real
    && resolveExecutable("claw-introuvable", { PATH: installDir }) === null, `${found} / ${real}`);

  const copies = path.join(root, "copies");
  const first = workingCopy(installed, copies);
  const future = new Date(Date.now() + 5000);
  fs.utimesSync(installed, future, future);
  const second = workingCopy(installed, copies);
  check("copie de travail refaite pour une nouvelle version, l'ancienne retirée",
    first !== second && fs.existsSync(second) && !fs.existsSync(first) && fs.readdirSync(copies).length === 1,
    fs.readdirSync(copies).join(", "));

  let changes = 0;
  const stop = watchExecutable(installed, () => (changes += 1), 100);
  await new Promise((resolve) => setTimeout(resolve, 450));
  const quiet = changes;
  // Comme `cargo install` : nouveau fichier écrit à côté, puis mis à la place de l'ancien.
  const incoming = installed + ".nouveau";
  fs.copyFileSync(claw, incoming);
  const later = new Date(Date.now() + 10000);
  fs.utimesSync(incoming, later, later);
  fs.renameSync(incoming, installed);
  await new Promise((resolve) => setTimeout(resolve, 700));
  stop();
  check("remplacement de l'exécutable détecté une seule fois", quiet === 0 && changes === 1, `${quiet} puis ${changes}`);
  mock.child.kill();
  console.log(`\n${results.filter(Boolean).length}/${results.length} vérifications réussies`);
  process.exit(results.every(Boolean) ? 0 : 1);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
