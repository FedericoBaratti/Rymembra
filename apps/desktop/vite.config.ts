import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// La porta è fissa e `strictPort` è acceso apposta: `devUrl` in tauri.conf.json
// punta a 5173, e un Vite che ripiega su 5174 perché la porta è occupata
// aprirebbe una finestra bianca senza dire perché.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // Il crate Rust lo ricostruisce Tauri: guardarlo anche da qui farebbe
      // ricaricare la pagina a ogni salvataggio di un file che non la riguarda.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // La finestra incorpora una versione nota di WebView2: non serve
    // trasformare per browser che non incontreremo mai.
    target: "chrome120",
  },
});
