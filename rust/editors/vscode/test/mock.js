// Lance le faux modèle de rust/scripts/mock-model.js et attend son port.
"use strict";

const path = require("path");
const { spawn } = require("child_process");

function startMockModel(logPath) {
  const script = path.join(__dirname, "..", "..", "..", "scripts", "mock-model.js");
  const child = spawn(process.execPath, [script, "--log", logPath], { stdio: ["ignore", "pipe", "inherit"] });
  return new Promise((resolve, reject) => {
    let output = "";
    child.stdout.on("data", (data) => {
      output += data;
      const match = output.match(/PORT (\d+)/);
      if (match) resolve({ child, port: Number(match[1]), baseUrl: `http://127.0.0.1:${match[1]}/v1` });
    });
    child.on("exit", (code) => reject(new Error(`le faux modèle s'est arrêté (code ${code})`)));
  });
}

module.exports = { startMockModel };
