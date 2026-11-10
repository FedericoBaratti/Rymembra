/**
 * Su che cosa gira la pagina: il computer o il telefono.
 *
 * Lo dice la CLI di Tauri al momento della costruzione (`TAURI_ENV_PLATFORM`,
 * vedi `envPrefix` in `vite.config.ts`), e non il nucleo a ogni avvio: la
 * risposta non cambia mentre l'app è aperta, e averla subito vuol dire che il
 * primo disegno è già quello giusto, senza un fotogramma con la barra del
 * titolo di Windows su un telefono.
 *
 * Fuori da Tauri — `npm run dev:web`, le prove con `node --test` — la
 * variabile non c'è, e la pagina si comporta come sul computer, che è com'era
 * prima che il telefono esistesse.
 */

/** Il nome della piattaforma, come lo scrive la CLI di Tauri. */
const nome: string = import.meta.env?.TAURI_ENV_PLATFORM ?? "";

/** La pagina è stata costruita per Android. */
export const ANDROID = nome === "android";

/**
 * La pagina gira su un telefono (o un tablet): niente finestra da trascinare,
 * niente vassoio, niente scorciatoie da tastiera, niente updater.
 */
export const MOBILE = ANDROID || nome === "ios";
