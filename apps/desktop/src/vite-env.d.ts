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

// I nomi delle lingue, raccolti a build time da `_nome` in ogni
// `src/lingue/*.json` (`vite.config.ts`): per codice. Serve a `lingue/index.ts`
// per scrivere il selettore senza caricare i dizionari.
declare module "virtual:lingue-nomi" {
  const nomi: Record<string, string>;
  export default nomi;
}
