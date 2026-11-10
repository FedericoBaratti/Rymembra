/// <reference types="vite/client" />

// La piattaforma per cui la CLI di Tauri ha costruito la pagina: «windows»,
// «android». Manca fuori da Tauri. Vedi `piattaforma.ts`.
interface ImportMetaEnv {
  readonly TAURI_ENV_PLATFORM?: string;
}

// Serve per due cose sole, ed entrambe stanno in `lingue/index.ts`:
// `import.meta.glob`, che è ciò che rende l'elenco delle lingue uguale al
// contenuto della cartella, e `import.meta.env.DEV`, che decide se avvisare in
// console per una chiave che manca.
