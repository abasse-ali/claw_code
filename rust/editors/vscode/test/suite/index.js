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
  "claw.acceptEdit",
  "claw.acceptEditAlways",
  "claw.rejectEdit",
  "claw.showChanges",
  "claw.showSessionChanges",
  "claw.interrupt",
  "claw.openInBrowser",
  "claw.reload",
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

/** Même fichier : sous Windows, la lettre du lecteur change de casse d'un outil à l'autre. */
function samePath(left, right) {
  const normalize = (file) => (process.platform === "win32" ? path.resolve(file).toLowerCase() : path.resolve(file));
  return normalize(left) === normalize(right);
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

  // L'en-tête seul ne suffit pas : la page doit réellement se charger dans le
  // panneau (son script se connecte alors au flux d'événements).
  await step("la page de claw se charge réellement dans le panneau", async () => {
    await waitFor("connexion de la page du panneau", async () => {
      const answer = await api.server.request("GET", "/api/viewers");
      return answer && answer.viewers >= 1;
    }, 30000);
  });

  /** L'agent a fini son tour : un message, un changement de modèle ou de mode
   * ne passe qu'au repos (sinon 409 : l'agent est déjà en train de travailler). */
  const idle = async () => {
    for (;;) {
      await waitFor("agent au repos", () => api.busy === false, 60000);
      await new Promise((resolve) => setTimeout(resolve, 600));
      if (api.busy === false) return;
    }
  };

  await step("la barre d'état affiche les crédits OpenRouter", async () => {
    await waitFor("crédits dans la barre d'état", () => api.statusText.includes("8,25 $"), 20000);
  });

  await step("les crédits se mettent à jour en temps réel pendant que l'agent travaille", async () => {
    // Modèle gratuit « OpenRouter » servi par le faux modèle : chaque étape
    // décompte une requête gratuite, poussée aussitôt à la barre d'état.
    await idle();
    await api.server.request("POST", "/api/model", { value: "mock/codeur:free" });
    await api.server.send("Réponds bonjour");
    await waitFor(
      "décompte d'une requête gratuite (sans attendre l'actualisation périodique)",
      () => /· 67[0-7] gratuites/.test(api.statusText),
      4000,
    );
    await idle();
    await api.server.request("POST", "/api/model", { value: "ollama:mock" });
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

  // Modification proposée : diff de l'éditeur, accepter ou refuser.
  const notes = path.join(process.env.CLAW_TEST_WORKSPACE, "notes");
  const essai = path.join(notes, "essai.txt");
  const diffTabs = (side, scheme) => vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .filter((tab) => tab.input instanceof vscode.TabInputTextDiff && tab.input[side].scheme === scheme);
  const proposal = () => waitFor(
    "diff de la modification proposée",
    () => diffTabs("modified", "claw-proposed").length === 1 && api.pendingEdits.length === 1,
    40000,
  );
  const setMode = async (mode) => {
    await idle();
    await api.server.request("POST", "/api/permissions", { value: mode });
  };
  const proposalClosed = () => waitFor(
    "diff fermé",
    () => diffTabs("modified", "claw-proposed").length === 0 && api.pendingEdits.length === 0,
    15000,
  );

  await step("une modification proposée s'ouvre dans un diff de l'éditeur, sans être écrite", async () => {
    await setMode("prompt");
    await api.server.send("Crée ce fichier [mock:write notes/essai.txt=bonjour de claw]");
    await proposal();
    const edit = api.pendingEdits[0];
    assert.ok(samePath(edit.path, essai), edit.path);
    const proposed = await vscode.workspace.openTextDocument(edit.modified);
    assert.strictEqual(proposed.getText(), "bonjour de claw\n");
    assert.ok(!fs.existsSync(essai), "fichier écrit avant l'accord");
  });

  await step("Accepter applique la modification et ferme le diff", async () => {
    await vscode.commands.executeCommand("claw.acceptEdit");
    await waitFor("fichier écrit", () => fs.existsSync(essai) && fs.readFileSync(essai, "utf8") === "bonjour de claw\n", 20000);
    await proposalClosed();
  });

  await step("Refuser laisse le fichier absent et transmet la consigne à l'agent", async () => {
    const refus = path.join(notes, "refus.txt");
    await api.server.send("Et celui-ci [mock:write notes/refus.txt=non merci]");
    await proposal();
    await vscode.commands.executeCommand("claw.rejectEdit", undefined, "pas ce fichier");
    await proposalClosed();
    await waitFor("consigne reçue par le modèle", () => modelMessages().includes("pas ce fichier"), 20000);
    assert.ok(!fs.existsSync(refus), "fichier écrit malgré le refus");
  });

  // Problèmes vus par l'éditeur : ceux d'une collection de diagnostics d'essai.
  const problems = vscode.languages.createDiagnosticCollection("essai-claw");
  const problem = (line, column, message, severity) => {
    const diagnostic = new vscode.Diagnostic(new vscode.Range(line, column, line, column + 4), message, severity);
    diagnostic.source = "essai";
    return diagnostic;
  };

  await step("l'agent lit les problèmes signalés par l'éditeur (outil EditorDiagnostics)", async () => {
    problems.set(demo, [
      problem(1, 4, "variable inconnue (essai)", vscode.DiagnosticSeverity.Error),
      problem(0, 0, "style douteux (essai)", vscode.DiagnosticSeverity.Warning),
    ]);
    await api.server.send('Quelles erreurs vois-tu ? [mock:tool EditorDiagnostics={"path":"src/demo.rs"}]');
    await waitFor("problèmes reçus par le modèle", () => {
      const received = modelMessages();
      return received.includes("src/demo.rs:2:5: error: variable inconnue (essai) [essai]")
        && received.includes("src/demo.rs:1:1: warning: style douteux (essai) [essai]");
    }, 40000);
    assert.ok(fs.readFileSync(process.env.CLAW_TEST_LOG, "utf8").includes('"name":"EditorDiagnostics"'), "outil non proposé au modèle");
  });

  await step("les erreurs de l'éditeur dans un fichier modifié repartent à l'agent", async () => {
    await setMode("workspace-write");
    problems.set(vscode.Uri.file(essai), [problem(0, 0, "mot interdit (essai)", vscode.DiagnosticSeverity.Error)]);
    await api.server.send("Réécris-le [mock:write notes/essai.txt=deuxième version]");
    await waitFor("erreurs transmises au modèle", () => {
      const received = modelMessages();
      return received.includes("VS Code reports 1 error(s) in the files you changed") && received.includes("mot interdit (essai)");
    }, 40000);
    problems.clear();
  });

  await step("les fichiers modifiés par le tour s'ouvrent en diff (avant / maintenant)", async () => {
    await waitFor("fichier réécrit", () => fs.readFileSync(essai, "utf8") === "deuxième version\n", 30000);
    await vscode.commands.executeCommand("claw.showChanges");
    await waitFor("diff avant / maintenant", () => diffTabs("original", "claw-before").length === 1, 15000);
    assert.strictEqual(api.changedFiles.length, 1);
    assert.ok(samePath(api.changedFiles[0], essai), api.changedFiles[0]);
    const tab = diffTabs("original", "claw-before")[0];
    const before = await vscode.workspace.openTextDocument(tab.input.original);
    assert.strictEqual(before.getText(), "bonjour de claw\n");
    assert.strictEqual(fs.readFileSync(essai, "utf8"), "deuxième version\n");
    await vscode.window.tabGroups.close(tab, true);
    await setMode("read-only");
  });

  // Mise à jour de claw pendant que le panneau est ouvert : l'exécutable
  // installé doit pouvoir être remplacé (il n'est pas verrouillé par le
  // panneau), puis le panneau se recharge seul sur la même session.
  await step("le panneau se recharge seul après une mise à jour de claw", async () => {
    await waitFor("session connue de l'extension", () => Boolean(api.sessionId), 15000);
    await waitFor("agent au repos", async () => {
      const sessions = await api.server.request("GET", "/api/sessions");
      return sessions.sessions.some((session) => session.id === api.sessionId && session.messages >= 2);
    }, 30000);
    const before = api.server;
    const session = api.sessionId;
    const reloads = api.reloadCount;
    const installed = process.env.CLAW_TEST_INSTALLED;
    const incoming = `${installed}.nouveau`;
    fs.copyFileSync(process.env.CLAW_TEST_SOURCE, incoming);
    const later = new Date(Date.now() + 60000);
    fs.utimesSync(incoming, later, later);
    fs.renameSync(incoming, installed);
    await waitFor("nouveau claw serve", () => api.reloadCount === reloads + 1 && api.server && api.server !== before && api.server.running, 40000);
    assert.ok(!before.running, "l'ancien claw serve tourne encore");
    assert.ok(api.panel && api.panel.webview.html.includes(api.server.url), "le panneau ne pointe pas vers le nouveau claw serve");
    await waitFor("page rechargée dans le panneau", async () => {
      const answer = await api.server.request("GET", "/api/viewers");
      return answer && answer.viewers >= 1;
    }, 30000);
    await waitFor("session reprise", () => api.sessionId === session, 15000);
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
