import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Su un telefono vero la pagina non si chiede a `localhost`: `localhost` è il
// telefono. `tauri android dev` mette in `TAURI_DEV_HOST` l'indirizzo di questo
// computer sulla rete, e Vite deve ascoltare lì — anche per il ricaricamento
// a caldo, che passa da un WebSocket tutto suo. Senza la variabile (Windows,
// l'emulatore) resta tutto com'era.
const ospite = process.env.TAURI_DEV_HOST;

// La porta è fissa e `strictPort` è acceso apposta: `devUrl` in tauri.conf.json
// punta a 5173, e un Vite che ripiega su 5174 perché la porta è occupata
// aprirebbe una finestra bianca senza dire perché.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // `TAURI_ENV_PLATFORM` e le sue sorelle le imposta la CLI di Tauri prima di
  // lanciare Vite: è così che la pagina sa se è stata costruita per Windows o
  // per Android (`piattaforma.ts`) senza chiederlo al nucleo a ogni avvio.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 5173,
    strictPort: true,
    host: ospite || false,
    ...(ospite ? { hmr: { protocol: "ws", host: ospite, port: 5183 } } : {}),
    watch: {
      // Il crate Rust lo ricostruisce Tauri: guardarlo anche da qui farebbe
      // ricaricare la pagina a ogni salvataggio di un file che non la riguarda.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Il pavimento vero, non uno scelto a occhio. La finestra è WebView2 nella
    // versione «evergreen», che Windows aggiorna da sé insieme a Edge: un
    // browser vecchio qui non arriva mai, e trasformare il codice per lui è
    // lavoro buttato. Il numero però deve dire quel che il codice chiede già,
    // e il codice chiede Chromium 125: `<ViewTransition>` e `addTransitionType`
    // di React, insieme al compilatore delle skin, scrivono
    // `::view-transition-old(.classe)` e i tipi di transizione (125);
    // `light-dark()` nei token (123); `scrollbar-width` nel foglio (121);
    // `@starting-style` e `text-wrap: pretty` (117); le animazioni legate allo
    // scorrimento (115). Era `chrome120`, che prometteva una compatibilità che
    // l'app non ha: su un 120 si aprirebbe, con i passaggi fra le viste spenti
    // e i colori a metà.
    //
    // Su Android la finestra è la WebView di sistema, che Google Play aggiorna
    // da sé come WebView2 su Windows: lo stesso pavimento, e su un telefono con
    // una WebView più vecchia di così l'app si apre con le stesse mancanze.
    target: "chrome125",
  },
});
