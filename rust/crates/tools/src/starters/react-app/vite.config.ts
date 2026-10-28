import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// La page construite va dans docs/ : c'est elle que GitHub Pages publie et que Claw mesure.
// Adresses relatives : le site vit dans un sous-dossier (utilisateur.github.io/depot/).
export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  build: { outDir: "docs", emptyOutDir: true },
});
