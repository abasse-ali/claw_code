// Extension VS Code de Claw Code : l'agent dans un panneau de l'éditeur,
// envoi de la sélection ou du fichier courant, modifications proposées dans
// un diff de l'éditeur (accepter ou refuser), et problèmes vus par l'éditeur
// transmis à l'agent.
"use strict";

const path = require("path");
const vscode = require("vscode");
const {
  ClawServer,
  selectionMessage,
  resolveExecutable,
  workingCopy,
  watchExecutable,
  workspacePath,
  diagnosticsAnswer,
} = require("./claw-client");

// Documents virtuels : contenu proposé par l'agent, fichier d'avant ses modifications.
const PROPOSED_SCHEME = "claw-proposed";
const BEFORE_SCHEME = "claw-before";
// Nom sous lequel l'extension se déclare à claw serve.
const EDITOR_NAME = "VS Code";

let server = null;
let panel = null;
let statusItem = null;
let creditsTimer = null;
let credits = null;
// Dossier de l'extension où garder la copie de travail de claw.
let storageDir = null;
// Surveillance de l'exécutable installé, dernier état reçu de claw serve.
let stopWatching = null;
let lastStatus = null;
// Une nouvelle version de claw attend la fin du tour en cours.
let updatePending = false;
let reloading = false;
let reloadCount = 0;
// Contenu des documents virtuels, par adresse.
const virtualDocuments = new Map();
// Modifications proposées qui attendent une réponse, par identifiant de demande.
const pendingEdits = new Map();
let acceptItem = null;
let rejectItem = null;
// Fichiers modifiés par le dernier tour (bouton de la barre d'état).
let changesItem = null;
let lastChanges = [];
let changeVersion = 0;

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
    nativeDiff: config.get("nativeDiff") !== false,
    editorDiagnostics: config.get("editorDiagnostics") !== false,
  };
}

function setStatus(text, tooltip) {
  if (!statusItem) return;
  statusItem.text = credits ? `${text} · ${credits.balance}` : text;
  statusItem.tooltip = credits ? `${tooltip}\nOpenRouter : ${credits.detail}` : tooltip;
}

/** Affiche des crédits reçus (événement en direct ou lecture de /api/credits). */
function applyCredits(running, data) {
  if (data && data.available && data.balance_text) {
    const free = data.free ? ` · ${data.free.remaining} gratuites` : "";
    const session = data.session && data.session.free + data.session.paid
      ? `\nCette session : ${data.session.free} requête(s) gratuite(s), ${data.session.paid} payante(s)`
      : "";
    credits = {
      balance: `${data.balance_text}${free}`,
      detail: `${data.text}${session}\nMis à jour à ${new Date().toLocaleTimeString()}`,
    };
  } else {
    credits = null;
  }
  if (running.running) setStatus("$(hubot) Claw", `Claw actif dans ${running.options.cwd}`);
}

async function refreshCredits(running) {
  try {
    applyCredits(running, await running.request("GET", "/api/credits"));
  } catch (_) {
    // affichage précédent conservé
  }
}

/**
 * Suit `claw serve` en direct : crédits OpenRouter dans la barre d'état (une
 * lecture par minute sert de secours), état de l'agent, modifications
 * proposées, demandes de diagnostics et fichiers modifiés par un tour.
 */
function startEvents(running) {
  stopCredits();
  refreshCredits(running);
  running.subscribe(
    (event) => {
      if (event.type === "credits") applyCredits(running, event.credits);
      if (running !== server) return;
      if (event.type === "status" && event.status) {
        lastStatus = event.status;
        // Tour terminé : une mise à jour en attente peut s'appliquer.
        maybeReload();
      } else if (event.type === "permission" && event.proposal) {
        if (settings().nativeDiff) showProposal(running, event).catch(reportError);
      } else if (event.type === "permission_resolved") {
        closeProposal(event.id);
      } else if (event.type === "editor_sync") {
        // Sans réponse, Claw poursuit seul après son délai d'attente.
        answerDiagnostics(running, event).catch(() => undefined);
      } else if (event.type === "turn_done") {
        refreshChanges(running);
      } else if (event.type === "reset") {
        closeAllProposals();
        setChanges([]);
      }
    },
    { editor: settings().editorDiagnostics ? EDITOR_NAME : undefined },
  );
  creditsTimer = setInterval(() => refreshCredits(running), 60000);
}

function stopCredits() {
  if (creditsTimer) clearInterval(creditsTimer);
  creditsTimer = null;
  credits = null;
}

function reportError(error) {
  vscode.window.showErrorMessage(`Claw : ${error && error.message ? error.message : error}`);
}

/**
 * Exécutable installé et celui à lancer. Sous Windows le panneau lance une
 * copie : l'exécutable installé reste libre d'être remplacé par une mise à jour.
 */
function launchPaths(clawPath) {
  const installed = resolveExecutable(clawPath);
  if (!installed) return { installed: null, run: clawPath };
  if (process.platform === "win32" && storageDir) {
    try {
      return { installed, run: workingCopy(installed, path.join(storageDir, "bin")) };
    } catch (_) {
      // Copie impossible (disque plein, droits) : l'exécutable installé est lancé tel quel.
    }
  }
  return { installed, run: installed };
}

/** Démarre `claw serve` pour le dossier ouvert (une seule fois). */
async function ensureServer() {
  if (server && server.running && server.url) return server;
  const cwd = workspaceFolder();
  if (!cwd) throw new Error("Ouvrez d'abord un dossier de projet.");
  const config = settings();
  const launch = launchPaths(config.clawPath);
  const started = new ClawServer({ ...config, clawPath: launch.run, cwd });
  server = started;
  started.onExit((code, stderr) => {
    // Un serveur remplacé (rechargement) ne touche plus à l'affichage.
    if (server !== started) return;
    stopCredits();
    closeAllProposals();
    setChanges([]);
    setStatus("$(circle-slash) Claw", "Claw est arrêté");
    if (code !== 0 && code !== null) {
      vscode.window.showErrorMessage(`Claw s'est arrêté (code ${code}). ${stderr.split("\n").slice(-3).join(" ")}`);
    }
  });
  if (stopWatching) stopWatching();
  stopWatching = launch.installed
    ? watchExecutable(launch.installed, () => {
        updatePending = true;
        maybeReload();
      })
    : null;
  setStatus("$(sync~spin) Claw", "Démarrage de Claw…");
  await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: "Démarrage de Claw…" },
    () => server.start(),
  );
  setStatus("$(hubot) Claw", `Claw actif dans ${cwd}`);
  startEvents(server);
  return server;
}

/**
 * Applique une mise à jour de claw dès que l'agent est au repos : sinon elle
 * attend la fin du tour en cours (signalée par l'événement d'état suivant).
 */
async function maybeReload() {
  if (!updatePending || reloading) return;
  if (!server || !server.running) {
    // Rien ne tourne : le prochain démarrage prendra la nouvelle version.
    updatePending = false;
    return;
  }
  if (lastStatus && lastStatus.busy) {
    setStatus("$(sync) Claw", "Mise à jour de Claw en attente : le panneau se rechargera à la fin du tour en cours");
    return;
  }
  updatePending = false;
  try {
    await reloadServer("Claw a été mis à jour : le panneau s'est rechargé");
  } catch (error) {
    vscode.window.showErrorMessage(`Claw : rechargement impossible après la mise à jour (${error.message})`);
  }
}

/**
 * Relance `claw serve` (avec l'exécutable installé du moment), reprend la
 * session en cours et recharge le panneau.
 */
async function reloadServer(message) {
  if (reloading) return;
  reloading = true;
  try {
    const sessionId = lastStatus && lastStatus.session_id;
    const previous = server;
    server = null;
    lastStatus = null;
    stopCredits();
    closeAllProposals();
    setChanges([]);
    if (previous) previous.stop();
    const running = await ensureServer();
    if (sessionId) {
      try {
        await running.request("POST", "/api/resume", { value: sessionId });
      } catch (_) {
        // Session vide, jamais enregistrée : la nouvelle session suffit.
      }
    }
    if (panel) panel.webview.html = panelHtml(running.url, running.port);
    reloadCount += 1;
    vscode.window.setStatusBarMessage(message, 6000);
  } finally {
    reloading = false;
  }
}

/** Thème de l'éditeur, transmis à la page de Claw (`?theme=`). */
function editorTheme() {
  const kind = vscode.window.activeColorTheme.kind;
  return kind === vscode.ColorThemeKind.Light || kind === vscode.ColorThemeKind.HighContrastLight ? "light" : "dark";
}

function panelHtml(url, port) {
  const origin = `http://127.0.0.1:${port}`;
  const src = `${url}&embed=1&theme=${editorTheme()}`;
  return `<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; frame-src ${origin}; style-src 'unsafe-inline';">
<style>html, body, iframe { margin: 0; padding: 0; width: 100%; height: 100%; border: 0; overflow: hidden; }</style>
</head>
<body><iframe src="${src}" title="Claw Code" allow="clipboard-read; clipboard-write"></iframe></body>
</html>`;
}

async function openPanel() {
  const running = await ensureServer();
  if (panel) {
    panel.reveal(vscode.ViewColumn.Beside, true);
    return running;
  }
  panel = vscode.window.createWebviewPanel("claw", "Claw Code", { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true }, {
    enableScripts: true,
    retainContextWhenHidden: true,
  });
  panel.iconPath = new vscode.ThemeIcon("hubot");
  panel.webview.html = panelHtml(running.url, running.port);
  panel.onDidDispose(() => (panel = null));
  return running;
}

// --- Modifications proposées : diff de l'éditeur, accepter ou refuser ---------

/** Colonne où ouvrir un diff : jamais celle du panneau de Claw, qui doit rester visible. */
function diffColumn() {
  return panel && panel.viewColumn === vscode.ViewColumn.One ? vscode.ViewColumn.Two : vscode.ViewColumn.One;
}

/** Onglets de diff dont le côté modifié est `uri`. */
function diffTabs(uri) {
  const target = uri.toString();
  return vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .filter((tab) => tab.input instanceof vscode.TabInputTextDiff && tab.input.modified.toString() === target);
}

function latestEdit() {
  let latest = null;
  for (const edit of pendingEdits.values()) latest = edit;
  return latest;
}

/** Boutons Accepter et Refuser de la barre d'état, visibles tant qu'une modification attend. */
function refreshEditItems() {
  const edit = latestEdit();
  vscode.commands.executeCommand("setContext", "claw.editPending", Boolean(edit));
  if (!acceptItem || !rejectItem) return;
  if (!edit) {
    acceptItem.hide();
    rejectItem.hide();
    return;
  }
  const name = path.basename(edit.path);
  acceptItem.tooltip = `Claw : appliquer la modification proposée de ${name}`;
  rejectItem.tooltip = `Claw : refuser la modification proposée de ${name}`;
  acceptItem.show();
  rejectItem.show();
}

/**
 * Claw demande l'autorisation d'écrire un fichier : le changement s'ouvre dans
 * un diff de l'éditeur (fichier actuel à gauche, proposition à droite). La
 * réponse se donne ici (Accepter, Refuser) ou dans le panneau de Claw.
 */
async function showProposal(running, event) {
  const proposal = event.proposal;
  const file = vscode.Uri.file(proposal.path);
  const modified = file.with({ scheme: PROPOSED_SCHEME, query: `id=${event.id}` });
  virtualDocuments.set(modified.toString(), proposal.after);
  let original = file;
  if (!proposal.exists) {
    // Fichier à créer : comparé à un document vide.
    original = file.with({ scheme: PROPOSED_SCHEME, query: `id=${event.id}&vide=1` });
    virtualDocuments.set(original.toString(), "");
  }
  pendingEdits.set(event.id, { id: event.id, running, path: proposal.path, original, modified });
  refreshEditItems();
  const name = path.basename(proposal.path);
  const title = proposal.exists ? `${name} : modification proposée par Claw` : `${name} : nouveau fichier proposé par Claw`;
  await vscode.commands.executeCommand("vscode.diff", original, modified, title, {
    preview: false,
    preserveFocus: true,
    viewColumn: diffColumn(),
  });
}

/** La demande a reçu sa réponse (ici, dans le panneau, ou par délai) : le diff se ferme. */
async function closeProposal(id) {
  const edit = pendingEdits.get(id);
  if (!edit) return;
  pendingEdits.delete(id);
  refreshEditItems();
  const tabs = diffTabs(edit.modified);
  if (tabs.length) {
    try {
      await vscode.window.tabGroups.close(tabs, true);
    } catch (_) {
      // onglet déjà fermé par l'utilisateur
    }
  }
  virtualDocuments.delete(edit.modified.toString());
  virtualDocuments.delete(edit.original.toString());
}

function closeAllProposals() {
  for (const id of [...pendingEdits.keys()]) closeProposal(id);
}

/** Modification visée : celle du diff d'où vient la commande, sinon du diff actif, sinon la dernière. */
function pickEdit(uri) {
  const fromUri = (candidate) => {
    if (!candidate || candidate.scheme !== PROPOSED_SCHEME) return null;
    const id = Number(new URLSearchParams(candidate.query).get("id"));
    return pendingEdits.get(id) || null;
  };
  const active = vscode.window.tabGroups.activeTabGroup.activeTab;
  const activeUri = active && active.input instanceof vscode.TabInputTextDiff ? active.input.modified : null;
  return fromUri(uri) || fromUri(activeUri) || latestEdit();
}

async function answerEdit(uri, decision, feedback) {
  const edit = pickEdit(uri);
  if (!edit) {
    vscode.window.showInformationMessage("Claw n'attend aucune réponse sur une modification.");
    return;
  }
  const body = { id: edit.id, decision };
  if (feedback) body.feedback = feedback;
  try {
    await edit.running.request("POST", "/api/permission", body);
  } catch (error) {
    // 404 : la réponse a déjà été donnée dans le panneau de Claw.
    if (!/^404/.test(error.message)) throw error;
  }
  await closeProposal(edit.id);
}

/** Refus, avec une consigne facultative pour l'agent (`feedback` fourni : pas de saisie). */
async function rejectEdit(uri, feedback) {
  if (!pickEdit(uri)) {
    vscode.window.showInformationMessage("Claw n'attend aucune réponse sur une modification.");
    return;
  }
  let reason = feedback;
  if (typeof reason !== "string") {
    reason = await vscode.window.showInputBox({
      prompt: "Refuser cette modification : consigne pour Claw (facultative)",
      placeHolder: "Entrée pour refuser, Échap pour annuler",
    });
    if (reason === undefined) return;
  }
  await answerEdit(uri, "deny", reason.trim());
}

// --- Problèmes vus par l'éditeur ----------------------------------------------

/**
 * Attend que les analyses de l'éditeur se stabilisent : plus de changement de
 * diagnostics (sur `targets`, ou partout si la liste est vide) pendant
 * `quietMs`, sans dépasser `maxMs`.
 */
function diagnosticsSettled(targets, quietMs, maxMs) {
  return new Promise((resolve) => {
    const wanted = new Set(targets.map((uri) => uri.toString()));
    let quiet = null;
    let limit = null;
    let subscription = null;
    const finish = () => {
      clearTimeout(quiet);
      clearTimeout(limit);
      if (subscription) subscription.dispose();
      resolve();
    };
    const rearm = () => {
      clearTimeout(quiet);
      quiet = setTimeout(finish, quietMs);
    };
    subscription = vscode.languages.onDidChangeDiagnostics((event) => {
      if (!wanted.size || event.uris.some((uri) => wanted.has(uri.toString()))) rearm();
    });
    limit = setTimeout(finish, maxMs);
    rearm();
  });
}

/**
 * Claw demande les problèmes que l'éditeur voit dans des fichiers (tout
 * l'espace de travail si aucun n'est nommé). Les fichiers sont ouverts en
 * arrière-plan pour que leurs serveurs de langage les analysent.
 */
async function answerDiagnostics(running, event) {
  const cwd = running.options.cwd;
  const targets = (event.paths || []).map((file) => vscode.Uri.file(path.isAbsolute(file) ? file : path.join(cwd, file)));
  await Promise.all(
    targets.map((uri) => Promise.resolve(vscode.workspace.openTextDocument(uri)).catch(() => undefined)),
  );
  await diagnosticsSettled(targets, targets.length ? 700 : 300, targets.length ? 6000 : 2000);
  const entries = targets.length
    ? targets.map((uri) => [uri.fsPath, vscode.languages.getDiagnostics(uri)])
    : vscode.languages
        .getDiagnostics()
        .filter(([uri, list]) => uri.scheme === "file" && list.length)
        .map(([uri, list]) => [uri.fsPath, list]);
  await running.request("POST", "/api/editor/diagnostics", { id: event.id, files: diagnosticsAnswer(cwd, entries) });
}

// --- Fichiers modifiés par l'agent --------------------------------------------

function setChanges(files) {
  lastChanges = files;
  if (!changesItem) return;
  if (!files.length) {
    changesItem.hide();
    return;
  }
  changesItem.text = `$(diff) ${files.length}`;
  changesItem.tooltip = files.length === 1
    ? "Claw : 1 fichier modifié au dernier tour (cliquer pour voir le diff)"
    : `Claw : ${files.length} fichiers modifiés au dernier tour (cliquer pour voir les diffs)`;
  changesItem.show();
}

async function refreshChanges(running) {
  try {
    const answer = await running.request("GET", "/api/changes?scope=turn");
    if (running === server) setChanges((answer && answer.files) || []);
  } catch (_) {
    // affichage précédent conservé
  }
}

/** Diff d'un fichier : son contenu avant les modifications de l'agent, et maintenant. */
async function openChange(file) {
  const uri = vscode.Uri.file(file.path);
  if (!file.comparable) {
    vscode.window.showInformationMessage(`${path.basename(file.path)} : fichier binaire ou trop volumineux pour un diff.`);
    if (file.exists) await vscode.window.showTextDocument(uri, { viewColumn: diffColumn(), preview: true });
    return;
  }
  changeVersion += 1;
  const before = uri.with({ scheme: BEFORE_SCHEME, query: `v=${changeVersion}` });
  virtualDocuments.set(before.toString(), file.before || "");
  let current = uri;
  if (!file.exists) {
    current = uri.with({ scheme: BEFORE_SCHEME, query: `v=${changeVersion}&absent=1` });
    virtualDocuments.set(current.toString(), "");
  }
  await vscode.commands.executeCommand("vscode.diff", before, current, `${path.basename(file.path)} : avant Claw / maintenant`, {
    preview: true,
    viewColumn: diffColumn(),
  });
}

/**
 * Montre ce que l'agent a modifié (`scope` : `turn` pour le dernier tour,
 * `session` pour toute la session). Un seul fichier s'ouvre directement ;
 * sinon une liste permet de choisir. `only` (chemin) ouvre ce fichier-là.
 */
async function showChanges(scope, only) {
  if (!server || !server.running) {
    vscode.window.showInformationMessage("Claw n'est pas démarré.");
    return;
  }
  const running = server;
  const answer = await running.request("GET", `/api/changes?scope=${scope}`);
  let files = (answer && answer.files) || [];
  if (scope === "turn") setChanges(files);
  if (typeof only === "string") {
    const wanted = path.resolve(running.options.cwd, only).toLowerCase();
    files = files.filter((file) => path.resolve(file.path).toLowerCase() === wanted);
  }
  if (!files.length) {
    vscode.window.showInformationMessage(
      scope === "turn" ? "Claw n'a modifié aucun fichier au dernier tour." : "Claw n'a modifié aucun fichier dans cette session.",
    );
    return;
  }
  let chosen = files[0];
  if (files.length > 1) {
    const picked = await vscode.window.showQuickPick(
      files.map((file) => ({
        label: workspacePath(running.options.cwd, file.path),
        description: !file.existed ? "créé" : file.exists ? "modifié" : "supprimé",
        file,
      })),
      { placeHolder: scope === "turn" ? "Fichiers modifiés par Claw au dernier tour" : "Fichiers modifiés par Claw dans cette session" },
    );
    if (!picked) return;
    chosen = picked.file;
  }
  await openChange(chosen);
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

/** Bouton de la barre d'état, caché tant qu'il n'a rien à proposer. */
function actionItem(priority, text, command, warning) {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, priority);
  item.text = text;
  item.command = command;
  if (warning) item.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
  return item;
}

function activate(context) {
  storageDir = context.globalStorageUri ? context.globalStorageUri.fsPath : null;
  statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusItem.command = "claw.open";
  setStatus("$(hubot) Claw", "Ouvrir Claw");
  statusItem.show();
  acceptItem = actionItem(103, "$(check) Accepter", "claw.acceptEdit", true);
  rejectItem = actionItem(102, "$(close) Refuser", "claw.rejectEdit", true);
  changesItem = actionItem(101, "$(diff)", "claw.showChanges", false);
  const virtualProvider = { provideTextDocumentContent: (uri) => virtualDocuments.get(uri.toString()) || "" };
  // Le panneau suit le thème de l'éditeur (la page se recharge, la conversation reste).
  context.subscriptions.push(
    vscode.window.onDidChangeActiveColorTheme(() => {
      if (panel && server && server.running && server.url) panel.webview.html = panelHtml(server.url, server.port);
    }),
  );
  context.subscriptions.push(
    statusItem,
    acceptItem,
    rejectItem,
    changesItem,
    vscode.workspace.registerTextDocumentContentProvider(PROPOSED_SCHEME, virtualProvider),
    vscode.workspace.registerTextDocumentContentProvider(BEFORE_SCHEME, virtualProvider),
    vscode.commands.registerCommand("claw.open", guard(openPanel)),
    vscode.commands.registerCommand("claw.ask", guard(ask)),
    vscode.commands.registerCommand("claw.sendSelection", guard(sendSelection)),
    vscode.commands.registerCommand("claw.askAboutFile", guard(askAboutFile)),
    vscode.commands.registerCommand("claw.acceptEdit", guard((uri) => answerEdit(uri, "once"))),
    vscode.commands.registerCommand("claw.acceptEditAlways", guard((uri) => answerEdit(uri, "always"))),
    vscode.commands.registerCommand("claw.rejectEdit", guard(rejectEdit)),
    vscode.commands.registerCommand("claw.showChanges", guard((only) => showChanges("turn", only))),
    vscode.commands.registerCommand("claw.showSessionChanges", guard((only) => showChanges("session", only))),
    vscode.commands.registerCommand("claw.interrupt", guard(async () => server && server.running && server.interrupt())),
    vscode.commands.registerCommand("claw.openInBrowser", guard(async () => {
      const running = await ensureServer();
      await vscode.env.openExternal(vscode.Uri.parse(running.url));
    })),
    vscode.commands.registerCommand("claw.reload", guard(async () => {
      if (server && server.running) await reloadServer("Panneau Claw rechargé");
      else await openPanel();
    })),
    vscode.commands.registerCommand("claw.stop", () => {
      stopCredits();
      closeAllProposals();
      setChanges([]);
      updatePending = false;
      if (stopWatching) stopWatching();
      stopWatching = null;
      if (server) server.stop();
      if (panel) panel.dispose();
    }),
    {
      dispose: () => {
        if (stopWatching) stopWatching();
        if (server) server.stop();
      },
    },
  );
  // État exposé aux autres extensions et aux tests d'intégration.
  return {
    get server() {
      return server;
    },
    get panel() {
      return panel;
    },
    get statusText() {
      return statusItem ? statusItem.text : "";
    },
    /** Session en cours dans claw serve (`null` tant que l'état n'est pas reçu). */
    get sessionId() {
      return lastStatus ? lastStatus.session_id : null;
    },
    /** L'agent travaille (`null` tant que l'état n'est pas reçu). */
    get busy() {
      return lastStatus ? Boolean(lastStatus.busy) : null;
    },
    /** Nombre de rechargements (mise à jour de claw ou commande claw.reload). */
    get reloadCount() {
      return reloadCount;
    },
    /** Demandes de modification affichées dans un diff, en attente de réponse. */
    get pendingEdits() {
      return [...pendingEdits.values()].map((edit) => ({ id: edit.id, path: edit.path, modified: edit.modified }));
    },
    /** Fichiers modifiés par le dernier tour (bouton de la barre d'état). */
    get changedFiles() {
      return lastChanges.map((file) => file.path);
    },
  };
}

function deactivate() {
  if (stopWatching) stopWatching();
  if (server) server.stop();
}

module.exports = { activate, deactivate };
