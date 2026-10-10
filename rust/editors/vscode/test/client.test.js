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
const {
  ClawServer,
  selectionMessage,
  resolveExecutable,
  workingCopy,
  watchExecutable,
  clawDiagnostic,
  workspacePath,
  diagnosticsAnswer,
} = require("../claw-client");
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
  // CLAW_NO_USER_ENV : l'essai ne reprend pas les clés du compte de la machine.
  const env = {
    OLLAMA_BASE_URL: mock.baseUrl,
    CLAW_CONFIG_HOME: path.join(root, "claw-home"),
    CLAW_NO_CLAUDE_CONFIG: "1",
    CLAW_NO_USER_ENV: "1",
  };

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

  // Éditeur relié : Claw lui demande les problèmes d'un fichier, l'outil les rend au modèle.
  const wait = async (label, predicate, attempts = 80) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await predicate()) return true;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    console.log(`       délai dépassé : ${label}`);
    return false;
  };
  const events = [];
  const asked = [];
  server.subscribe(
    (event) => {
      events.push(event);
      if (event.type !== "editor_sync") return;
      asked.push(event.paths);
      const problems = [
        { range: { start: { line: 6, character: 2 } }, severity: 0, message: "type incompatible (essai)", source: "essai", code: { value: 2322 } },
        { range: { start: { line: 0, character: 0 } }, severity: 1, message: "import inutile (essai)" },
      ];
      const files = diagnosticsAnswer(cwd, [[path.join(cwd, "src", "app.ts"), problems]]);
      server.request("POST", "/api/editor/diagnostics", { id: event.id, files }).catch(() => undefined);
    },
    { editor: "Éditeur d'essai" },
  );
  assert.deepStrictEqual(clawDiagnostic({ range: { start: { line: 6, character: 2 } }, severity: 0, message: "m", source: "ts", code: { value: 2322 } }),
    { line: 7, column: 3, severity: "error", message: "m", source: "ts", code: "2322" });
  assert.strictEqual(workspacePath(cwd, path.join(cwd, "src", "app.ts")), "src/app.ts");
  assert.strictEqual(workspacePath(cwd, path.join(root, "ailleurs.ts")), path.join(root, "ailleurs.ts"));
  const count = (type) => events.filter((event) => event.type === type).length;
  /** Envoie un message et attend la fin du tour qu'il ouvre. */
  const turn = async (text, during) => {
    const done = count("turn_done");
    await server.send(text);
    if (during) await during();
    return wait(`fin du tour « ${text.slice(0, 30)} »`, () => count("turn_done") > done, 160);
  };
  // Le premier message a été envoyé avant l'abonnement : attendre que l'agent soit au repos.
  await wait("agent au repos", () => {
    const statuses = events.filter((event) => event.type === "status" && event.status);
    return statuses.length > 0 && !statuses[statuses.length - 1].status.busy;
  });
  await new Promise((resolve) => setTimeout(resolve, 300));
  const problemLine = "src/app.ts:7:3: error: type incompatible (essai) [essai 2322]";
  await turn('Quelles erreurs ? [mock:tool EditorDiagnostics={"path":"src/app.ts","severity":"error"}]');
  const diagnosed = fs.readFileSync(logPath, "utf8").includes(problemLine);
  received = fs.readFileSync(logPath, "utf8");
  check("l'éditeur relié répond à l'outil EditorDiagnostics (erreurs seules)",
    diagnosed && asked.length === 1 && asked[0][0] === "src/app.ts" && !received.split("EditorDiagnostics=").pop().includes("import inutile (essai)")
      && received.includes('"name":"EditorDiagnostics"'),
    `${JSON.stringify(asked)} ${received.slice(-300)}`);

  // Questions de l'agent : proposées avec des choix, les réponses reviennent au modèle.
  const asked2 = JSON.stringify({
    questions: [
      { question: "Quel moteur d'OCR ?", header: "OCR", options: [{ label: "EasyOCR (Recommandé)", description: "Plus simple" }, { label: "manga-ocr" }] },
      { question: "Quelles langues cibles ?", multiSelect: true, options: ["Français", "Anglais"] },
    ],
  });
  let questionEvent = { questions: [] };
  await turn(`Propose une architecture [mock:tool AskUserQuestion=${asked2}]`, async () => {
    await wait("questions de l'agent", () => events.some((event) => event.type === "question"));
    questionEvent = events.find((event) => event.type === "question") || questionEvent;
    if (questionEvent.id !== undefined) {
      await server.request("POST", "/api/question", {
        id: questionEvent.id,
        answers: [
          { question: "Quel moteur d'OCR ?", answer: "EasyOCR (Recommandé)" },
          { question: "Quelles langues cibles ?", answer: "Français, Anglais" },
        ],
      });
    }
  });
  received = fs.readFileSync(logPath, "utf8");
  const questionResolved = events.find((event) => event.type === "question_resolved") || {};
  check("l'agent pose ses questions avec des choix, les réponses lui reviennent",
    questionEvent.questions.length === 2 && questionEvent.questions[0].header === "OCR"
      && questionEvent.questions[0].options[0].description === "Plus simple" && questionEvent.questions[1].multi_select === true
      && questionEvent.questions[1].options.map((option) => option.label).join() === "Français,Anglais"
      && questionResolved.dismissed === false && questionResolved.answers.length === 2
      && received.includes("Français, Anglais") && received.includes('"name":"AskUserQuestion"'),
    JSON.stringify([questionEvent, questionResolved]).slice(0, 500));

  // Demande de permission : la modification proposée accompagne la demande, rien n'est écrit avant l'accord.
  await server.request("POST", "/api/permissions", { value: "prompt" });
  const target = path.join(cwd, "notes", "essai.txt");
  let permission = { proposal: {} };
  let untouched = false;
  await turn("Crée ce fichier [mock:write notes/essai.txt=bonjour de claw]", async () => {
    await wait("demande de permission", () => events.some((event) => event.type === "permission" && event.proposal));
    permission = events.find((event) => event.type === "permission" && event.proposal) || permission;
    untouched = !fs.existsSync(target);
    if (permission.id !== undefined) await server.request("POST", "/api/permission", { id: permission.id, decision: "once" });
  });
  check("la demande de permission porte le contenu proposé, sans rien écrire",
    untouched && permission.proposal.after === "bonjour de claw\n" && permission.proposal.exists === false
      && path.resolve(permission.proposal.path || "") === path.resolve(target),
    JSON.stringify(permission.proposal));
  const written = fs.existsSync(target) && fs.readFileSync(target, "utf8") === "bonjour de claw\n";
  const turnChanges = await server.request("GET", "/api/changes?scope=turn");
  check("le fichier accepté est écrit et listé parmi les modifications du tour",
    written && turnChanges.files.length === 1 && turnChanges.files[0].existed === false && turnChanges.files[0].exists === true
      && path.resolve(turnChanges.files[0].path) === path.resolve(target),
    JSON.stringify(turnChanges));

  // Demande refusée : l'appel reçoit quand même un résultat. Sans lui, la page
  // et l'éditeur le montraient « en cours » indéfiniment.
  const refusedFile = path.join(cwd, "notes", "refuse.txt");
  const isRefusedRequest = (event) => event.type === "permission" && event.proposal && /refuse\.txt$/.test(event.proposal.path || "");
  const resultsBefore = count("tool_result");
  await turn("Crée ce fichier [mock:write notes/refuse.txt=non]", async () => {
    await wait("demande de permission à refuser", () => events.some(isRefusedRequest));
    const request = events.find(isRefusedRequest);
    if (request) await server.request("POST", "/api/permission", { id: request.id, decision: "deny" });
  });
  const refusal = events.filter((event) => event.type === "tool_result").slice(resultsBefore).pop() || {};
  check("un appel refusé reçoit un résultat d'erreur et n'écrit rien",
    refusal.is_error === true && refusal.name === "write_file" && /a refusé l'outil/.test(refusal.output || "") && !fs.existsSync(refusedFile),
    JSON.stringify(refusal));

  // Après une modification, les erreurs que l'éditeur voit dans le fichier repartent au modèle.
  await server.request("POST", "/api/permissions", { value: "workspace-write" });
  asked.length = 0;
  const notices = count("notice");
  await turn("Réécris-le [mock:write notes/essai.txt=deuxième version]");
  received = fs.readFileSync(logPath, "utf8");
  const sentBack = events.filter((event) => event.type === "notice").slice(notices)
    .some((event) => event.text.includes("signale 1 erreur(s) dans les fichiers modifiés"));
  check("les erreurs de l'éditeur dans un fichier modifié repartent au modèle",
    sentBack && asked.length === 1 && path.resolve(asked[0][0]) === path.resolve(target) && received.includes("Éditeur d'essai reports 1 error(s)")
      && fs.readFileSync(target, "utf8") === "deuxième version\n",
    `${JSON.stringify(asked)} ${JSON.stringify(events.filter((event) => ["notice", "info", "error", "turn_done", "tool_call"].includes(event.type)).slice(-12))}`);

  // Le fournisseur du modèle lâche en cours de réponse (504). Claw ne change pas de modèle
  // de lui-même : le même modèle reprend une fois, puis la question est posée.
  const asking = new ClawServer({
    clawPath: claw,
    cwd,
    model: "ollama:panne",
    permissionMode: "read-only",
    env: { ...env, CLAW_FALLBACK_MODELS: "ollama:mock" },
  });
  await asking.start();
  const askingEvents = [];
  asking.subscribe((event) => askingEvents.push(event));
  await wait("abonnement", () => askingEvents.some((event) => event.type === "status"));
  await asking.send("Réponds bonjour malgré la panne");
  const questions = () => askingEvents.filter((event) => event.type === "model_question");
  // Premier essai automatique : personne ne répond, le même modèle reprend seul après 3 s.
  await wait("premier essai automatique", () => questions().length >= 1, 160);
  await wait("deuxième essai automatique annoncé", () => questions().length >= 2, 160);
  const [firstQuestion, question] = questions();
  const autoResolved = askingEvents.find((event) => event.type === "model_question_resolved" && event.id === firstQuestion.id) || {};
  const announced = askingEvents.some((event) => event.type === "notice"
    && event.text === "ollama:panne : le fournisseur ne répond pas (erreur 504). Nouvel essai automatique dans 3 s (essai 1 sur 12)");
  check("après une panne, le même modèle réessaie de lui-même, sans intervention",
    announced && firstQuestion.model === "ollama:panne" && firstQuestion.auto_retry_secs === 3 && firstQuestion.attempt === 1 && firstQuestion.attempts === 12
      && autoResolved.decision === "nouvel essai automatique" && question.auto_retry_secs === 10 && question.attempt === 2
      && question.suggestions.includes("ollama:mock") && !askingEvents.some((event) => event.type === "assistant_text"),
    JSON.stringify(askingEvents.filter((event) => ["notice", "model_question", "model_question_resolved", "error"].includes(event.type)).slice(-5)));
  // L'utilisateur ouvre le choix d'un modèle : l'essai automatique l'attend, puis il choisit.
  await asking.request("POST", "/api/model-question", { id: question.id, decision: "hold" });
  await new Promise((resolve) => setTimeout(resolve, 600));
  await asking.request("POST", "/api/model-question", { id: question.id, decision: "switch", model: "ollama:mock" });
  await wait("fin du tour avec le modèle choisi", () => askingEvents.some((event) => event.type === "turn_done"), 160);
  await wait("modèle de la session", () => {
    const statuses = askingEvents.filter((event) => event.type === "status" && event.status);
    return statuses.length > 0 && statuses[statuses.length - 1].status.model === "ollama:mock" && !statuses[statuses.length - 1].status.busy;
  });
  const lastStatus = askingEvents.filter((event) => event.type === "status" && event.status).pop() || { status: {} };
  const resolved = askingEvents.find((event) => event.type === "model_question_resolved" && event.id === question.id) || {};
  check("le modèle choisi prend la suite et devient celui de la session",
    askingEvents.filter((event) => event.type === "assistant_text").map((event) => event.text).join("").includes("Réponse du modèle factice")
      && resolved.decision === "modèle choisi : ollama:mock" && lastStatus.status.model === "ollama:mock"
      && !askingEvents.some((event) => event.type === "error"),
    JSON.stringify([resolved, lastStatus.status.model]));
  asking.stop();

  // Panne passagère : le même modèle reprend seul et le tour se termine sans aucune intervention.
  const flaky = new ClawServer({ clawPath: claw, cwd, model: "ollama:panne-une-fois", permissionMode: "read-only", env });
  await flaky.start();
  const flakyEvents = [];
  flaky.subscribe((event) => flakyEvents.push(event));
  await wait("abonnement", () => flakyEvents.some((event) => event.type === "status"));
  await flaky.send("Réponds bonjour");
  await wait("fin du tour après une panne passagère", () => flakyEvents.some((event) => event.type === "turn_done"), 160);
  const flakyStatus = flakyEvents.filter((event) => event.type === "status" && event.status).pop() || { status: {} };
  check("une panne passagère se résout seule : même modèle, aucune intervention",
    flakyEvents.filter((event) => event.type === "model_question").length === 1
      && flakyEvents.filter((event) => event.type === "assistant_text").map((event) => event.text).join("").includes("Réponse du modèle factice")
      && flakyStatus.status.model === "ollama:panne-une-fois" && !flakyEvents.some((event) => event.type === "error"),
    JSON.stringify(flakyEvents.filter((event) => ["notice", "model_question", "model_question_resolved", "error", "turn_done"].includes(event.type)).slice(-5)));
  flaky.stop();

  // Panne après l'annonce d'un appel d'outil : la réponse est redemandée, et
  // l'appel annoncé par la réponse abandonnée ne reste pas affiché « en cours ».
  const cut = new ClawServer({ clawPath: claw, cwd, model: "ollama:panne-une-fois-outil", permissionMode: "read-only", env });
  await cut.start();
  const cutEvents = [];
  cut.subscribe((event) => cutEvents.push(event));
  await wait("abonnement", () => cutEvents.some((event) => event.type === "status"));
  await cut.send('Cherche [mock:tool glob_search={"pattern":"notes/*.txt"}]');
  await wait("fin du tour après une réponse coupée", () => cutEvents.some((event) => event.type === "turn_done"), 160);
  const cutOrder = cutEvents
    .filter((event) => ["tool_call", "reply_discarded", "tool_result"].includes(event.type))
    .map((event) => event.type)
    .join(" ");
  check("une réponse coupée après un appel d'outil retire cet appel avant le nouvel essai",
    cutOrder === "tool_call reply_discarded tool_call tool_result" && !cutEvents.some((event) => event.type === "error"),
    cutOrder);
  cut.stop();

  // Fournisseur qui se fige au milieu d'une réponse : après le délai
  // d'inactivité, la requête est abandonnée puis relancée, sans intervention.
  const frozen = new ClawServer({
    clawPath: claw,
    cwd,
    model: "ollama:fige-une-fois",
    permissionMode: "read-only",
    env: { ...env, CLAW_IDLE_TIMEOUT: "2" },
  });
  await frozen.start();
  const frozenEvents = [];
  frozen.subscribe((event) => frozenEvents.push(event));
  await wait("abonnement", () => frozenEvents.some((event) => event.type === "status"));
  await frozen.send("Réponds bonjour");
  await wait("fin du tour après une réponse figée", () => frozenEvents.some((event) => event.type === "turn_done"), 160);
  check("un fournisseur qui se fige ne bloque plus le tour : requête abandonnée puis relancée",
    frozenEvents.filter((event) => event.type === "assistant_text").map((event) => event.text).join("").includes("Réponse du modèle factice")
      && frozenEvents.some((event) => event.type === "notice" && /nouvelle tentative|Nouvel essai/.test(event.text))
      && !frozenEvents.some((event) => event.type === "error"),
    JSON.stringify(frozenEvents.filter((event) => ["notice", "error", "model_question", "turn_done"].includes(event.type)).slice(-4)).slice(0, 500));
  frozen.stop();

  // Appel d'outil que le fournisseur ne termine pas (le nom arrive, pas les
  // arguments) : jamais exécuté, et jamais soumis à l'autorisation de l'utilisateur.
  const startAsking = async (model) => {
    const instance = new ClawServer({ clawPath: claw, cwd, model, permissionMode: "prompt", env });
    await instance.start();
    const seen = [];
    instance.subscribe((event) => seen.push(event));
    await wait("abonnement", () => seen.some((event) => event.type === "status"));
    return { instance, seen };
  };
  const of = (seen, type) => seen.filter((event) => event.type === type);

  const once = await startAsking("ollama:coupe-une-fois");
  const onceFile = path.join(cwd, "notes", "coupe.txt");
  await once.instance.send("Crée ce fichier [mock:write notes/coupe.txt=entier]");
  await wait("demande d'autorisation du nouvel essai", () => of(once.seen, "permission").length > 0, 160);
  const onceRequest = of(once.seen, "permission")[0] || { proposal: {} };
  if (onceRequest.id !== undefined) await once.instance.request("POST", "/api/permission", { id: onceRequest.id, decision: "once" });
  await wait("fin du tour après un appel coupé", () => of(once.seen, "turn_done").length > 0, 160);
  check("un appel d'outil coupé est redemandé au modèle, et seul l'appel complet est soumis",
    of(once.seen, "permission").length === 1 && (onceRequest.proposal || {}).after === "entier\n"
      && of(once.seen, "reply_discarded").length === 1 && of(once.seen, "notice").some((event) => /coupée avant la fin d'un appel d'outil/.test(event.text))
      && fs.existsSync(onceFile) && fs.readFileSync(onceFile, "utf8") === "entier\n" && of(once.seen, "error").length === 0,
    JSON.stringify(once.seen.filter((event) => ["permission", "notice", "reply_discarded", "error", "tool_result"].includes(event.type)).slice(-6)).slice(0, 600));
  once.instance.stop();

  const always = await startAsking("ollama:coupe");
  await always.instance.send("Crée ce fichier [mock:write notes/jamais.txt=rien]");
  await wait("question sur le modèle", () => of(always.seen, "model_question").length > 0, 160);
  const stuckQuestion = of(always.seen, "model_question")[0] || {};
  if (stuckQuestion.id !== undefined) await always.instance.request("POST", "/api/model-question", { id: stuckQuestion.id, decision: "stop" });
  await wait("fin du tour arrêté", () => of(always.seen, "turn_done").length > 0, 160);
  check("des appels toujours coupés : deux nouveaux essais, puis la question à l'utilisateur, sans rien autoriser",
    /s'arrêtent avant la fin des appels d'outils/.test(stuckQuestion.reason || "") && of(always.seen, "permission").length === 0
      && of(always.seen, "notice").filter((event) => /coupée avant la fin d'un appel d'outil/.test(event.text)).length === 2
      && !fs.existsSync(path.join(cwd, "notes", "jamais.txt")),
    JSON.stringify([stuckQuestion, of(always.seen, "notice").map((event) => event.text), of(always.seen, "permission").length]).slice(0, 600));
  always.instance.stop();

  const limited = await startAsking("ollama:limite");
  await limited.instance.send("Crée ce fichier [mock:write notes/limite.txt=contenu trop long pour la sortie]");
  await wait("fin du tour après la limite de sortie", () => of(limited.seen, "turn_done").length > 0, 160);
  const limitResult = of(limited.seen, "tool_result").pop() || {};
  check("un appel coupé par la limite de sortie n'est ni autorisé ni exécuté, et le modèle apprend pourquoi",
    limitResult.is_error === true && /reached the output limit before the arguments of `write_file`/.test(limitResult.output || "")
      && of(limited.seen, "permission").length === 0 && of(limited.seen, "model_question").length === 0
      && !fs.existsSync(path.join(cwd, "notes", "limite.txt")),
    JSON.stringify([limitResult, of(limited.seen, "permission").length]).slice(0, 500));
  limited.instance.stop();

  // Avec "modelSwitch": "auto", la requête repart d'elle-même sur le modèle de secours.
  const fragile = new ClawServer({
    clawPath: claw,
    cwd,
    model: "ollama:panne",
    permissionMode: "read-only",
    env: { ...env, CLAW_FALLBACK_MODELS: "ollama:mock", CLAW_MODEL_SWITCH: "auto" },
  });
  await fragile.start();
  const fragileEvents = [];
  fragile.subscribe((event) => fragileEvents.push(event));
  await wait("abonnement", () => fragileEvents.some((event) => event.type === "status"));
  await fragile.send("Réponds bonjour malgré la panne");
  await wait("fin du tour après la panne", () => fragileEvents.some((event) => event.type === "turn_done"), 160);
  const switched = fragileEvents.find((event) => event.type === "notice" && event.text.includes("a cessé de répondre"));
  const answered = fragileEvents.filter((event) => event.type === "assistant_text").map((event) => event.text).join("");
  check("en mode automatique, la requête repart seule sur le modèle de secours",
    Boolean(switched) && switched.text === "ollama:panne a cessé de répondre (erreur 504) → la requête repart sur ollama:mock"
      && answered.includes("Réponse du modèle factice") && !fragileEvents.some((event) => event.type === "error"),
    JSON.stringify(fragileEvents.filter((event) => ["notice", "error", "assistant_text", "turn_done"].includes(event.type)).slice(-6)));
  fragile.stop();

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
