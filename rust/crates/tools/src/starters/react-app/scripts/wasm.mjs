// Construit le module Rust de wasm/ en WebAssembly (public/calc.wasm), avant chaque
// construction de l'appli. Sans dossier wasm/, il ne fait rien.
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, readFileSync } from "node:fs";

if (!existsSync("wasm/Cargo.toml")) process.exit(0);

const name = /^name\s*=\s*"([^"]+)"/m.exec(readFileSync("wasm/Cargo.toml", "utf8"))?.[1] ?? "calc";
const target = "wasm32-unknown-unknown";
const run = (command, args) => execFileSync(command, args, { stdio: "inherit", cwd: "wasm" });

try {
  try {
    execFileSync("rustup", ["target", "add", target], { stdio: "ignore" });
  } catch {
    // Sans rustup : la cible est peut-être déjà là.
  }
  run("cargo", ["build", "--release", "--target", target]);
} catch (error) {
  const built = `public/${name}.wasm`;
  if (existsSync(built)) {
    console.warn(`Le module Rust n'a pas pu être reconstruit (${error.message}) : ${built} d'avant est gardé.`);
    process.exit(0);
  }
  console.error(`Le module Rust ne se construit pas : installez Rust (rustup.rs) et la cible ${target}.`);
  process.exit(1);
}
copyFileSync(`wasm/target/${target}/release/${name}.wasm`, `public/${name}.wasm`);
console.log(`public/${name}.wasm construit.`);
