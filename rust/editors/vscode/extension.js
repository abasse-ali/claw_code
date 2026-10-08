// Extension VS Code de Claw Code : l'agent dans un panneau de l'éditeur, et
// envoi de la sélection ou du fichier courant.
"use strict";

const path = require("path");
const vscode = require("vscode");
const { ClawServer, selectionMessage } = require("./claw-client");

let server = null;
let panel = null;
let statusItem = null;

function workspaceFolder() {
  const editor = vscode.window.activeTextEditor;
  if (editor) {
    const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
    if (folder) return folder.uri.fsPath;
  }
  const folders = vscode.workspace.workspaceFolders;
  return folders && folders.length ? folders[0].uri.fsPath : undefined;
}

function settings() {
  const config = vscode.workspace.getConfiguration("claw");
  return {
    clawPath: config.get("path") || "claw",
    model: config.get("model") || undefined,
    permissionMode: config.get("permissionMode") || undefined,
  };
}

function setStatus(text, tooltip) {
  if (!statusItem) return;
  statusItem.text = text;
  statusItem.tooltip = tooltip;
}

/** Démarre `claw serve` pour le dossier ouvert (une seule fois). */
async function ensureServer() {
  if (server && server.running && server.url) return server;
  const cwd = workspaceFolder();
  if (!cwd) throw new Error("Ouvrez d'abord un dossier de projet.");
  server = new ClawServer({ ...settings(), cwd });
  server.onExit((code, stderr) => {
    setStatus("$(circle-slash) Claw", "Claw est arrêté");
    if (code !== 0 && code !== null) {
      vscode.window.showErrorMessage(`Claw s'est arrêté (code ${code}). ${stderr.split("\n").slice(-3).join(" ")}`);
    }
  });
  setStatus("$(sync~spin) Claw", "Démarrage de Claw…");
  await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: "Démarrage de Claw…" },
    () => server.start(),
  );
  setStatus("$(hubot) Claw", `Claw actif dans ${cwd}`);
  return server;
}

function panelHtml(url, port) {
  const origin = `http://127.0.0.1:${port}`;
  return `<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; frame-src ${origin}; style-src 'unsafe-inline';">
<style>html, body, iframe { margin: 0; padding: 0; width: 100%; height: 100%; border: 0; overflow: hidden; }</style>
</head>
<body><iframe src="${url}" title="Claw" allow="clipboard-read; clipboard-write"></iframe></body>
</html>`;
}

async function openPanel() {
  const running = await ensureServer();
  if (panel) {
    panel.reveal(vscode.ViewColumn.Beside, true);
    return running;
  }
  panel = vscode.window.createWebviewPanel("claw", "Claw", { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true }, {
    enableScripts: true,
    retainContextWhenHidden: true,
  });
  panel.iconPath = new vscode.ThemeIcon("hubot");
  panel.webview.html = panelHtml(running.url, running.port);
  panel.onDidDispose(() => (panel = null));
  return running;
}

async function sendSelection() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.selection.isEmpty) {
    vscode.window.showInformationMessage("Sélectionnez d'abord du code.");
    return;
  }
  const instruction = await vscode.window.showInputBox({
    prompt: "Que faire avec cette sélection ? (laisser vide pour la joindre seulement)",
    placeHolder: "ex. explique ce code, trouve le bug, refactorise…",
  });
  if (instruction === undefined) return;
  const running = await openPanel();
  const document = editor.document;
  const cwd = workspaceFolder() || path.dirname(document.uri.fsPath);
  const message = selectionMessage({
    instruction,
    relativePath: path.relative(cwd, document.uri.fsPath).replace(/\\/g, "/"),
    startLine: editor.selection.start.line + 1,
    endLine: editor.selection.end.line + 1,
    languageId: document.languageId,
    text: document.getText(editor.selection),
  });
  await running.send(message);
}

async function askAboutFile(uri) {
  const target = uri && uri.fsPath ? uri : vscode.window.activeTextEditor && vscode.window.activeTextEditor.document.uri;
  if (!target) {
    vscode.window.showInformationMessage("Ouvrez ou choisissez un fichier.");
    return;
  }
  const question = await vscode.window.showInputBox({
    prompt: `Votre demande à propos de ${path.basename(target.fsPath)}`,
    placeHolder: "ex. fais une revue de ce fichier",
  });
  if (!question) return;
  const running = await openPanel();
  const cwd = workspaceFolder() || path.dirname(target.fsPath);
  const relative = path.relative(cwd, target.fsPath).replace(/\\/g, "/");
  const mention = /\s/.test(relative) ? `@"${relative}"` : `@${relative}`;
  await running.send(`${question} ${mention}`);
}

async function ask() {
  const question = await vscode.window.showInputBox({ prompt: "Votre demande à Claw" });
  if (!question) return;
  const running = await openPanel();
  await running.send(question);
}

function guard(action) {
  return async (...args) => {
    try {
      await action(...args);
    } catch (error) {
      vscode.window.showErrorMessage(`Claw : ${error.message}`);
    }
  };
}

function activate(context) {
  statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusItem.command = "claw.open";
  setStatus("$(hubot) Claw", "Ouvrir Claw");
  statusItem.show();
  context.subscriptions.push(
    statusItem,
    vscode.commands.registerCommand("claw.open", guard(openPanel)),
    vscode.commands.registerCommand("claw.ask", guard(ask)),
    vscode.commands.registerCommand("claw.sendSelection", guard(sendSelection)),
    vscode.commands.registerCommand("claw.askAboutFile", guard(askAboutFile)),
    vscode.commands.registerCommand("claw.interrupt", guard(async () => server && server.running && server.interrupt())),
    vscode.commands.registerCommand("claw.openInBrowser", guard(async () => {
      const running = await ensureServer();
      await vscode.env.openExternal(vscode.Uri.parse(running.url));
    })),
    vscode.commands.registerCommand("claw.stop", () => {
      if (server) server.stop();
      if (panel) panel.dispose();
    }),
    { dispose: () => server && server.stop() },
  );
}

function deactivate() {
  if (server) server.stop();
}

module.exports = { activate, deactivate };
