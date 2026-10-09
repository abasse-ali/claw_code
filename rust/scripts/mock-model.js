#!/usr/bin/env node
// Faux modèle compatible OpenAI (chat/completions, SSE) pour les tests
// d'intégration : extension VS Code, action GitHub.
//
//   node mock-model.js [--port N] [--log fichier] [--write chemin=contenu] [--read chemin]
//                      [--tool nom=arguments-json]
//
// Affiche « PORT <n> » une fois prêt. Chaque requête est ajoutée au journal
// (une ligne JSON). Avec --write, le premier tour demande l'outil write_file,
// puis le modèle conclut quand il reçoit le résultat de l'outil. Avec --read,
// il demande read_file puis répond avec le contenu lu (sonde de `claw models bench`).
// Avec --tool, il appelle l'outil donné puis répond avec le début de son résultat.
//
// Un message de l'utilisateur peut aussi piloter le modèle, le temps de son tour :
//   [mock:write chemin=contenu]         demande write_file
//   [mock:tool Nom={"argument": 1}]     appelle cet outil, puis cite son résultat
"use strict";

const fs = require("fs");
const http = require("http");

function option(name) {
  const index = process.argv.indexOf(name);
  return index > 0 ? process.argv[index + 1] : undefined;
}

const logPath = option("--log");
const write = option("--write");
const read = option("--read");
const tool = option("--tool");
// --reply fichier : texte de la réponse finale (Markdown de démonstration).
const replyFile = option("--reply");
const port = Number(option("--port") || 0);
// Requêtes faites à des modèles « :free » : décomptées du quota fictif.
let freeRequests = 0;

function lastUserText(messages) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.role !== "user") continue;
    if (typeof message.content === "string") return message.content;
    if (Array.isArray(message.content)) {
      return message.content.map((part) => part.text || "").join("\n");
    }
  }
  return "";
}

function messageText(message) {
  if (typeof message.content === "string") return message.content;
  return Array.isArray(message.content) ? message.content.map((part) => part.text || "").join("\n") : "";
}

/**
 * Dernière consigne « [mock:…] » écrite par l'utilisateur, et si un résultat
 * d'outil l'a déjà suivie (le modèle conclut alors par du texte).
 */
function lastDirective(messages) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index].role !== "user") continue;
    const match = messageText(messages[index]).match(/\[mock:(write|tool) ([^\n]+)\]/);
    if (!match) continue;
    const separator = match[2].indexOf("=");
    return {
      kind: match[1],
      name: separator > 0 ? match[2].slice(0, separator) : match[2],
      value: separator > 0 ? match[2].slice(separator + 1) : "",
      answered: messages.slice(index + 1).some((later) => later.role === "tool"),
    };
  }
  return null;
}

let directiveCalls = 0;

function directiveReply(directive, messages) {
  if (!directive.answered) {
    directiveCalls += 1;
    const id = `call_mock_directive_${directiveCalls}`;
    return directive.kind === "write"
      ? { toolCall: { id, name: "write_file", arguments: JSON.stringify({ path: directive.name, content: `${directive.value}\n` }) } }
      : { toolCall: { id, name: directive.name, arguments: directive.value || "{}" } };
  }
  if (directive.kind === "write") return { text: "Fichier écrit par le modèle factice." };
  const result = [...messages].reverse().find((message) => message.role === "tool");
  return { text: `Résultat de l'outil : ${messageText(result).slice(0, 400)}` };
}

function reply(body) {
  const messages = body.messages || [];
  const directive = lastDirective(messages);
  if (directive) return directiveReply(directive, messages);
  const hasToolResult = messages.some((message) => message.role === "tool");
  if (tool && !hasToolResult) {
    const separator = tool.indexOf("=");
    const name = separator > 0 ? tool.slice(0, separator) : tool;
    const args = separator > 0 ? tool.slice(separator + 1) : "{}";
    return { toolCall: { id: "call_mock_tool", name, arguments: args } };
  }
  if (tool) {
    const result = [...messages].reverse().find((message) => message.role === "tool");
    const content = typeof result.content === "string" ? result.content : JSON.stringify(result.content);
    return { text: `Résultat de l'outil : ${content.slice(0, 400)}` };
  }
  if (read && !hasToolResult) {
    return { toolCall: { id: "call_mock_read", name: "read_file", arguments: JSON.stringify({ path: read }) } };
  }
  if (read) {
    const result = [...messages].reverse().find((message) => message.role === "tool");
    return { text: `Contenu lu : ${typeof result.content === "string" ? result.content : JSON.stringify(result.content)}` };
  }
  if (write && !hasToolResult) {
    const separator = write.indexOf("=");
    const path = separator > 0 ? write.slice(0, separator) : write;
    const content = separator > 0 ? write.slice(separator + 1) : "";
    return {
      toolCall: {
        id: "call_mock_1",
        name: "write_file",
        arguments: JSON.stringify({ path, content: `${content}\n` }),
      },
    };
  }
  const asked = lastUserText(messages).split("\n")[0].slice(0, 80);
  const text = replyFile
    ? fs.readFileSync(replyFile, "utf8")
    : hasToolResult ? "Fichier écrit par le modèle factice." : `Réponse du modèle factice à : ${asked}`;
  return { text };
}

function chunk(payload) {
  return `data: ${JSON.stringify(payload)}\n\n`;
}

function streamReply(response, model, answer) {
  response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" });
  const base = { id: "chatcmpl-mock", object: "chat.completion.chunk", created: Math.floor(Date.now() / 1000), model };
  if (answer.toolCall) {
    response.write(chunk({ ...base, choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: answer.toolCall.id, type: "function", function: { name: answer.toolCall.name, arguments: answer.toolCall.arguments } }] }, finish_reason: null }] }));
    response.write(chunk({ ...base, choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] }));
  } else {
    response.write(chunk({ ...base, choices: [{ index: 0, delta: { role: "assistant", content: answer.text }, finish_reason: null }] }));
    response.write(chunk({ ...base, choices: [{ index: 0, delta: {}, finish_reason: "stop" }] }));
  }
  response.write(chunk({ ...base, choices: [], usage: { prompt_tokens: 12, completion_tokens: 7, total_tokens: 19 } }));
  response.end("data: [DONE]\n\n");
}

function jsonReply(response, model, answer) {
  const message = answer.toolCall
    ? { role: "assistant", content: null, tool_calls: [{ id: answer.toolCall.id, type: "function", function: { name: answer.toolCall.name, arguments: answer.toolCall.arguments } }] }
    : { role: "assistant", content: answer.text };
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(JSON.stringify({
    id: "chatcmpl-mock",
    object: "chat.completion",
    model,
    choices: [{ index: 0, message, finish_reason: answer.toolCall ? "tool_calls" : "stop" }],
    usage: { prompt_tokens: 12, completion_tokens: 7, total_tokens: 19 },
  }));
}

const server = http.createServer((request, response) => {
  // Compte OpenRouter fictif (OPENROUTER_BASE_URL=http://127.0.0.1:<port>/v1) :
  // 10 $ achetés, 1,75 $ utilisés, 678/1000 requêtes gratuites restantes.
  if (request.method === "GET" && request.url.endsWith("/credits")) {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ data: { total_credits: 10, total_usage: 1.75 } }));
    return;
  }
  if (request.method === "GET" && request.url.endsWith("/key")) {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ data: { usage_daily: 0, limit_remaining: null, free_model_daily_requests: { used: 322 + freeRequests, limit: 1000, remaining: 678 - freeRequests } } }));
    return;
  }
  if (request.method === "GET" && request.url.endsWith("/models")) {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ object: "list", data: [{ id: "mock", object: "model" }] }));
    return;
  }
  if (request.method !== "POST" || !request.url.endsWith("/chat/completions")) {
    response.writeHead(404);
    response.end();
    return;
  }
  let raw = "";
  request.setEncoding("utf8");
  request.on("data", (data) => (raw += data));
  request.on("end", () => {
    let body;
    try {
      body = JSON.parse(raw);
    } catch (error) {
      response.writeHead(400);
      response.end(String(error));
      return;
    }
    if (logPath) fs.appendFileSync(logPath, `${JSON.stringify(body)}\n`);
    if (String(body.model || "").endsWith(":free")) freeRequests += 1;
    const answer = reply(body);
    const model = body.model || "mock";
    if (body.stream) streamReply(response, model, answer);
    else jsonReply(response, model, answer);
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`PORT ${server.address().port}`);
});
