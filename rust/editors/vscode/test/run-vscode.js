// Tests d'intégration dans un vrai VS Code, téléchargé à part avec un profil
// isolé (l'éditeur de l'utilisateur n'est pas touché). Un faux modèle répond à
// la place d'OpenRouter.
// Usage : node test/run-vscode.js <chemin de claw>
//   VSCODE_TEST_VERSION (défaut stable), VSCODE_TEST_CACHE (défaut .vscode-test)
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { runTests } = require("@vscode/test-electron");
const { startMockModel } = require("./mock");

async function main() {
  if (!process.argv[2]) throw new Error("usage : node test/run-vscode.js <chemin de claw>");
  const source = path.resolve(process.argv[2]);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "claw-vscode-"));
  // Exécutable « installé » propre à l'essai : la suite le remplace pour
  // simuler une mise à jour de claw.
  fs.mkdirSync(path.join(root, "installe"));
  const claw = path.join(root, "installe", path.basename(source));
  fs.copyFileSync(source, claw);
  const workspace = path.join(root, "projet");
  fs.mkdirSync(path.join(workspace, ".vscode"), { recursive: true });
  fs.mkdirSync(path.join(workspace, "src"), { recursive: true });
  fs.writeFileSync(path.join(workspace, "src", "demo.rs"), 'fn main() {\n    println!("bonjour");\n}\n');
  fs.writeFileSync(
    path.join(workspace, ".vscode", "settings.json"),
    JSON.stringify({ "claw.path": claw, "claw.model": "ollama:mock", "claw.permissionMode": "read-only" }, null, 2),
  );
  const logPath = path.join(root, "modele.jsonl");
  fs.writeFileSync(logPath, "");

  // Hérité d'un terminal de VS Code : ferait démarrer VS Code comme simple Node.
  delete process.env.ELECTRON_RUN_AS_NODE;
  const mock = await startMockModel(logPath);
  try {
    await runTests({
      version: process.env.VSCODE_TEST_VERSION || "stable",
      cachePath: process.env.VSCODE_TEST_CACHE || undefined,
      extensionDevelopmentPath: path.join(__dirname, ".."),
      extensionTestsPath: path.join(__dirname, "suite", "index.js"),
      launchArgs: [
        workspace,
        "--disable-extensions",
        `--user-data-dir=${path.join(root, "user-data")}`,
        `--extensions-dir=${path.join(root, "extensions")}`,
      ],
      extensionTestsEnv: {
        CLAW_TEST_LOG: logPath,
        CLAW_TEST_WORKSPACE: workspace,
        CLAW_TEST_SOURCE: source,
        CLAW_TEST_INSTALLED: claw,
        OLLAMA_BASE_URL: mock.baseUrl,
        // Compte OpenRouter fictif servi par le faux modèle (crédits en barre d'état).
        OPENROUTER_BASE_URL: mock.baseUrl,
        OPENROUTER_API_KEY: "sk-or-test-factice",
        CLAW_CONFIG_HOME: path.join(root, "claw-home"),
        CLAW_NO_CLAUDE_CONFIG: "1",
        // L'essai ne reprend pas les clés du compte de la machine.
        CLAW_NO_USER_ENV: "1",
      },
    });
  } finally {
    mock.child.kill();
  }
}

main().catch((error) => {
  console.error(error && error.message ? error.message : error);
  process.exit(1);
});
