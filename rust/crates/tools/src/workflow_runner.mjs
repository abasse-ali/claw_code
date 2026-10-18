// Exécuteur des workflows de Claw (lancé par Claw avec le modèle de
// permissions de Node : pas de fichiers, pas de processus).
//
// Le script tourne dans un contexte `vm` vide : ni process, ni require, ni
// import, ni génération de code ; il ne peut qu'appeler agent(), parallel(),
// pipeline(), phase() et log(), qui passent par Claw. Seul du texte traverse
// la frontière, jamais un objet de ce côté.
//
// Échange en lignes JSON : la première ligne reçue décrit le script ({meta,
// body, args}) ; les suivantes répondent aux demandes d'agents ({id, result}).
// Sur la sortie : meta, ready, agent, phase, log, puis done ou error.
import vm from "node:vm";
import { createInterface } from "node:readline";

const out = (message) => process.stdout.write(JSON.stringify(message) + "\n");
const bridge = (text) => {
  try { process.stdout.write(String(text) + "\n"); } catch (_) { /* Claw est parti */ }
};
const finish = (code) => setTimeout(() => process.exit(code), 20);

const BOOT = `(function (bridge, argsText) {
  "use strict";
  const pending = new Map();
  let next = 1;
  let current = null;
  const send = (message) => bridge(JSON.stringify(message));
  const show = (value) => typeof value === "string" ? value : JSON.stringify(value);
  const opt = (value) => value === undefined || value === null ? null : String(value);
  globalThis.args = argsText === null ? undefined : JSON.parse(argsText);
  globalThis.agent = (prompt, opts) => {
    opts = opts || {};
    if (typeof prompt !== "string" || !prompt.trim()) {
      return Promise.reject(new Error("agent(prompt, opts): prompt must be a non-empty string"));
    }
    const id = next++;
    return new Promise((resolve) => {
      pending.set(id, resolve);
      send({ op: "agent", id, prompt, label: opt(opts.label), phase: opts.phase == null ? current : String(opts.phase),
        schema: opts.schema == null ? null : opts.schema, model: opt(opts.model), agentType: opt(opts.agentType),
        isolation: opt(opts.isolation) });
    });
  };
  globalThis.parallel = (thunks) => Promise.all(Array.from(thunks, async (thunk) => {
    try { return await (typeof thunk === "function" ? thunk() : thunk); }
    catch (error) { send({ op: "log", text: "parallel: " + ((error && error.message) || error) }); return null; }
  }));
  globalThis.pipeline = (items, ...stages) => Promise.all(Array.from(items, async (item, index) => {
    let value = item;
    for (const stage of stages) {
      try { value = await stage(value, item, index); }
      catch (error) { send({ op: "log", text: "pipeline: " + ((error && error.message) || error) }); return null; }
      if (value === null || value === undefined) return null;
    }
    return value;
  }));
  globalThis.phase = (title) => { current = String(title); send({ op: "phase", title: current }); };
  globalThis.log = (...parts) => send({ op: "log", text: parts.map(show).join(" ") });
  globalThis.console = { log: globalThis.log, info: globalThis.log, warn: globalThis.log, error: globalThis.log };
  return (line) => {
    const message = JSON.parse(line);
    const resolve = pending.get(message.id);
    if (!resolve) return;
    pending.delete(message.id);
    resolve(message.result === undefined ? null : message.result);
  };
})`;

let deliver = null;
function start(init) {
  const context = vm.createContext({}, { codeGeneration: { strings: false, wasm: false } });
  let meta;
  try {
    meta = JSON.parse(vm.runInContext("JSON.stringify(" + init.meta + ")", context, { timeout: 1000 }));
  } catch (error) {
    out({ op: "error", text: "meta: " + ((error && error.message) || error) });
    return finish(1);
  }
  if (!meta || typeof meta.name !== "string" || !meta.name.trim() || typeof meta.description !== "string") {
    out({ op: "error", text: "meta must be a literal with a name and a description" });
    return finish(1);
  }
  out({ op: "meta", meta });
  let script;
  try {
    script = new vm.Script(
      "(async () => { const __result = await (async () => {\n" + init.body +
      "\n})(); return __result === undefined ? \"null\" : JSON.stringify(__result); })()",
      { filename: "workflow.js", lineOffset: -1 });
  } catch (error) {
    out({ op: "error", text: String((error && error.message) || error) });
    return finish(1);
  }
  deliver = vm.runInContext(BOOT, context)(bridge, init.args === undefined ? null : JSON.stringify(init.args));
  out({ op: "ready" });
  script.runInContext(context).then(
    (text) => { out({ op: "done", result: String(text) }); finish(0); },
    (error) => { out({ op: "error", text: String((error && (error.stack || error.message)) || error) }); finish(1); });
}

const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  if (deliver) { deliver(line); return; }
  try { start(JSON.parse(line)); } catch (error) { out({ op: "error", text: String(error) }); finish(1); }
});
lines.on("close", () => finish(0));
