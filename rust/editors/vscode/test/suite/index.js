// Suite exécutée dans l'hôte d'extensions de VS Code (voir run-vscode.js) :
// vraies commandes de l'extension, vrai `claw serve`, faux modèle.
"use strict";

const assert = require("assert");
const fs = require("fs");
const http = require("http");
const path = require("path");
const vscode = require("vscode");

const COMMANDS = [
  "claw.open",
  "claw.ask",
  "claw.sendSelection",
  "claw.askAboutFile",
  "claw.interrupt",
  "claw.openInBrowser",
  "claw.stop",
];

const results = [];

async function step(label, action) {
  try {
    await action();
    results.push(true);
    console.log(`OK     ${label}`);
  } catch (error) {
    results.push(false);
    console.log(`ÉCHEC  ${label}\n       ${(error && error.stack) || error}`);
  }
}

async function waitFor(label, predicate, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`délai dépassé : ${label}`);
}

/** Textes des messages reçus par le faux modèle. */
function modelMessages() {
  return fs
    .readFileSync(process.env.CLAW_TEST_LOG, "utf8")
    .split("\n")
    .filter(Boolean)
    .flatMap((line) => JSON.parse(line).messages || [])
    .map((message) =>
      typeof message.content === "string"
        ? message.content
        : (message.content || []).map((part) => part.text || "").join("\n"),
    )
    .join("\n");
}

/** Répond à la place de l'utilisateur à la prochaine boîte de saisie. */
function answerInputBox(answer) {
  const original = vscode.window.showInputBox;
  vscode.window.showInputBox = async () => answer;
  return () => {
    vscode.window.showInputBox = original;
  };
}

function fetchHeaders(port) {
  return new Promise((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port, path: "/", headers: { Host: `127.0.0.1:${port}` } }, (response) => {
        response.resume();
        response.on("end", () => resolve(response.headers));
      })
      .on("error", reject);
  });
}

async function run() {
  const extension = vscode.extensions.getExtension("claw-code.claw-code");
  const demo = vscode.Uri.file(path.join(process.env.CLAW_TEST_WORKSPACE, "src", "demo.rs"));
  let api;

  await step("extension trouvée et activée", async () => {
    assert.ok(extension, "extension claw-code.claw-code absente");
    api = await extension.activate();
    assert.ok(extension.isActive);
  });

  await step("toutes les commandes sont enregistrées", async () => {
    const registered = await vscode.commands.getCommands(true);
    for (const command of COMMANDS) assert.ok(registered.includes(command), command);
  });

  await step("claw.open démarre claw serve et ouvre le panneau", async () => {
    await vscode.commands.executeCommand("claw.open");
    assert.ok(api.server && api.server.running, "claw serve n'est pas lancé");
    assert.match(api.server.url, /^http:\/\/127\.0\.0\.1:\d+\/\?token=[A-Za-z0-9_-]+$/);
    assert.ok(api.panel, "panneau absent");
    assert.ok(api.panel.webview.html.includes(api.server.url), "le panneau ne pointe pas vers claw serve");
  });

  await step("la page de claw accepte d'être affichée dans le panneau", async () => {
    const headers = await fetchHeaders(api.server.port);
    const csp = headers["content-security-policy"] || "";
    assert.ok(csp.includes("frame-ancestors 'self' vscode-webview:"), csp);
    assert.ok(!headers["x-frame-options"], "X-Frame-Options interdit l'iframe");
  });

  await step("claw.sendSelection transmet la sélection au modèle", async () => {
    const document = await vscode.workspace.openTextDocument(demo);
    const editor = await vscode.window.showTextDocument(document);
    editor.selection = new vscode.Selection(new vscode.Position(0, 0), document.lineAt(1).range.end);
    const restore = answerInputBox("Explique ce code");
    try {
      await vscode.commands.executeCommand("claw.sendSelection");
    } finally {
      restore();
    }
    await waitFor("sélection reçue par le modèle", () => {
      const received = modelMessages();
      return received.includes("Explique ce code") && received.includes("src/demo.rs") && received.includes("lignes 1-2");
    });
  });

  await step("claw.askAboutFile joint le contenu du fichier", async () => {
    const restore = answerInputBox("Fais une revue");
    try {
      await vscode.commands.executeCommand("claw.askAboutFile", demo);
    } finally {
      restore();
    }
    await waitFor("fichier reçu par le modèle", () => {
      const received = modelMessages();
      return received.includes("Fais une revue") && received.includes('println!("bonjour")');
    });
  });

  await step("claw.interrupt répond sans erreur", async () => {
    await vscode.commands.executeCommand("claw.interrupt");
  });

  await step("claw.stop arrête claw serve et ferme le panneau", async () => {
    await vscode.commands.executeCommand("claw.stop");
    await waitFor("arrêt", () => !api.server.running && !api.panel, 10000);
  });

  const failed = results.filter((ok) => !ok).length;
  console.log(`\n${results.length - failed}/${results.length} vérifications réussies dans VS Code`);
  if (failed) throw new Error(`${failed} vérification(s) en échec`);
}

module.exports = { run };
