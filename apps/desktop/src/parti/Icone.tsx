/**
 * Le icone: quarantadue simboli disegnati apposta, montati una volta sola.
 *
 * # Perché uno sprite e non una libreria
 *
 * L'applicazione non ha dipendenze di componenti, di icone o di animazione, e
 * questo file è il motivo per cui può continuare a non averne: quarantadue
 * `<symbol>` in un `<defs>` nascosto costano meno di sei chilobyte e non portano
 * dietro un albero di pacchetti da aggiornare. La CSP è chiusa
 * (`default-src 'self'`), quindi un font di icone remoto non sarebbe nemmeno
 * caricabile.
 *
 * # Perché `currentColor` ovunque
 *
 * Nessuna icona porta un colore proprio: lo eredita dal testo che le sta
 * intorno, che a sua volta viene da un token della skin. È la regola per cui una
 * skin può ridisegnare tutto senza che nessuno debba ricordarsi di aggiornare
 * anche le icone.
 *
 * L'unica eccezione è `i-mark`, il marchio, che ha due archi di due colori
 * diversi: il secondo è il ciano della tavolozza di `plain`, e sta scritto come
 * `var(--skin-color-ciano)` invece che come letterale, perché un colore scritto
 * a mano nel markup è esattamente ciò che il motore delle skin esiste per
 * togliere.
 *
 * # La misura
 *
 * Tutti i simboli hanno `viewBox="0 0 20 20"` e `stroke-width` fra 1.5 e 2.6:
 * sono disegnati per essere resi fra 13 e 20 pixel, che sono le misure che il
 * disegno usa. Ingrandirli molto oltre assottiglia il tratto in proporzione.
 */

/** Il nome di un'icona. Chiuso: un refuso è un errore di compilazione. */
export type NomeIcona =
  | "i-home"
  | "i-album"
  | "i-artist"
  | "i-track"
  | "i-heart"
  | "i-heart-f"
  | "i-list"
  | "i-queue"
  | "i-search"
  | "i-settings"
  | "i-folder"
  | "i-play"
  | "i-pause"
  | "i-prev"
  | "i-next"
  | "i-shuffle"
  | "i-repeat"
  | "i-vol"
  | "i-vol-x"
  | "i-x"
  // I tre della barra del titolo, che qui è disegnata e non del sistema. Non
  // sono icone dell'applicazione: sono i glifi che Windows mette in quell'ordine
  // da trent'anni, ridisegnati col tratto delle altre invece che presi da un
  // carattere di sistema che una skin non potrebbe toccare.
  | "i-win-min"
  | "i-win-max"
  | "i-win-restore"
  | "i-plus"
  | "i-dots"
  | "i-grip"
  | "i-chev-d"
  | "i-chev-u"
  | "i-chev-l"
  | "i-chev-r"
  | "i-star"
  | "i-star-o"
  | "i-skin"
  | "i-cloud"
  | "i-import"
  | "i-scan"
  | "i-check"
  | "i-alert"
  | "i-expand"
  | "i-sort"
  | "i-eq"
  | "i-text"
  | "i-mark";

/** Le proprietà comuni ai simboli col tratto. */
const TRATTO = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

/** Le proprietà comuni ai simboli pieni. */
const PIENO = { fill: "currentColor" } as const;

/**
 * Il foglio dei simboli.
 *
 * Va montato **una volta** vicino alla radice: `<use href="#i-play">` cerca il
 * simbolo nello stesso documento, quindi basta che esista, non importa dove.
 */
export function Simboli() {
  return (
    <svg width="0" height="0" style={{ position: "absolute" }} aria-hidden="true" focusable="false">
      <defs>
        <symbol id="i-home" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.2 8.6 10 3.2l6.8 5.4v7.2a1 1 0 0 1-1 1h-3.4v-4.6H7.6v4.6H4.2a1 1 0 0 1-1-1Z" />
        </symbol>
        <symbol id="i-album" viewBox="0 0 20 20" {...TRATTO}>
          <circle cx="10" cy="10" r="6.8" />
          <circle cx="10" cy="10" r="1.9" />
        </symbol>
        <symbol id="i-artist" viewBox="0 0 20 20" {...TRATTO}>
          <circle cx="10" cy="6.6" r="3" />
          <path d="M4.2 16.6c0-3 2.6-4.7 5.8-4.7s5.8 1.7 5.8 4.7" />
        </symbol>
        <symbol id="i-track" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M7.4 14.6V5.2l8-1.6v9.2" />
          <circle cx="5.4" cy="14.6" r="2" />
          <circle cx="13.4" cy="12.8" r="2" />
        </symbol>
        <symbol id="i-heart" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M10 16.3S3.4 12.4 3.4 8a3.5 3.5 0 0 1 6.6-1.8A3.5 3.5 0 0 1 16.6 8c0 4.4-6.6 8.3-6.6 8.3Z" />
        </symbol>
        <symbol id="i-heart-f" viewBox="0 0 20 20" {...PIENO}>
          <path d="M10 16.3S3.4 12.4 3.4 8a3.5 3.5 0 0 1 6.6-1.8A3.5 3.5 0 0 1 16.6 8c0 4.4-6.6 8.3-6.6 8.3Z" />
        </symbol>
        <symbol id="i-list" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M4 6h12M4 10h12M4 14h8" />
        </symbol>
        <symbol id="i-queue" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.6 6h8.4M3.6 10h8.4M3.6 14h5.2" />
          <path d="M16.4 5.2v7.6" />
          <circle cx="14.8" cy="13.4" r="1.7" />
        </symbol>
        <symbol id="i-search" viewBox="0 0 20 20" {...TRATTO}>
          <circle cx="9" cy="9" r="5" />
          <path d="m12.8 12.8 3.6 3.6" />
        </symbol>
        <symbol id="i-settings" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.6 6.4h7.4M16.2 6.4h.2M3.6 13.6h2.4M11.2 13.6h5.2" />
          <circle cx="13.3" cy="6.4" r="2.3" />
          <circle cx="8.4" cy="13.6" r="2.3" />
        </symbol>
        <symbol id="i-folder" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.4 15.2V5.8a1 1 0 0 1 1-1h3.1l1.6 2h6.5a1 1 0 0 1 1 1v7.4a1 1 0 0 1-1 1H4.4a1 1 0 0 1-1-1Z" />
        </symbol>
        <symbol id="i-play" viewBox="0 0 20 20" {...PIENO}>
          <path d="M6.6 4.3 15.8 10l-9.2 5.7V4.3Z" />
        </symbol>
        <symbol id="i-pause" viewBox="0 0 20 20" {...PIENO}>
          <rect x="5.6" y="4.4" width="3.1" height="11.2" rx="1.1" />
          <rect x="11.3" y="4.4" width="3.1" height="11.2" rx="1.1" />
        </symbol>
        <symbol id="i-prev" viewBox="0 0 20 20" {...PIENO}>
          <path d="M15.2 4.8v10.4L7.4 10l7.8-5.2Z" />
          <rect x="4.2" y="4.8" width="1.9" height="10.4" rx=".95" />
        </symbol>
        <symbol id="i-next" viewBox="0 0 20 20" {...PIENO}>
          <path d="M4.8 4.8v10.4L12.6 10 4.8 4.8Z" />
          <rect x="13.9" y="4.8" width="1.9" height="10.4" rx=".95" />
        </symbol>
        <symbol id="i-shuffle" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.4 5.6h2.8l7.2 8.8h3.2M3.4 14.4h2.8l2.1-2.6M12 7.7l1.4-2.1h3.2" />
          <path d="m14.4 3.4 2.2 2.2-2.2 2.2M14.4 12.2l2.2 2.2-2.2 2.2" />
        </symbol>
        <symbol id="i-repeat" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M5.2 8.6V7.4a2 2 0 0 1 2-2h7.4" />
          <path d="m12.4 3.2 2.4 2.2-2.4 2.2" />
          <path d="M14.8 11.4v1.2a2 2 0 0 1-2 2H5.4" />
          <path d="m7.6 16.8-2.4-2.2 2.4-2.2" />
        </symbol>
        <symbol id="i-vol" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.6 7.8h2.7L10.2 4.8v10.4L6.3 12.2H3.6V7.8Z" />
          <path d="M12.9 7.7a3.3 3.3 0 0 1 0 4.6M15.2 5.6a6.2 6.2 0 0 1 0 8.8" />
        </symbol>
        <symbol id="i-vol-x" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M3.6 7.8h2.7L10.2 4.8v10.4L6.3 12.2H3.6V7.8Z" />
          <path d="m13.2 8.2 3.4 3.6M16.6 8.2l-3.4 3.6" />
        </symbol>
        <symbol id="i-x" viewBox="0 0 20 20" {...TRATTO}>
          <path d="m5.6 5.6 8.8 8.8M14.4 5.6l-8.8 8.8" />
        </symbol>
        <symbol id="i-win-min" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M5.4 10h9.2" />
        </symbol>
        <symbol id="i-win-max" viewBox="0 0 20 20" {...TRATTO}>
          <rect x="5.4" y="5.4" width="9.2" height="9.2" rx="1.6" />
        </symbol>
        {/* Due riquadri sfalsati: quello davanti è la finestra che torna
            piccola, quello dietro il posto che lascia. */}
        <symbol id="i-win-restore" viewBox="0 0 20 20" {...TRATTO}>
          <rect x="4.4" y="7.6" width="8" height="8" rx="1.5" />
          <path d="M7.6 7.6V6a1.6 1.6 0 0 1 1.6-1.6h4.4A1.6 1.6 0 0 1 15.6 6v4.4a1.6 1.6 0 0 1-1.6 1.6h-1.6" />
        </symbol>
        <symbol id="i-plus" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M10 4.4v11.2M4.4 10h11.2" />
        </symbol>
        <symbol id="i-dots" viewBox="0 0 20 20" {...PIENO}>
          <circle cx="5" cy="10" r="1.35" />
          <circle cx="10" cy="10" r="1.35" />
          <circle cx="15" cy="10" r="1.35" />
        </symbol>
        <symbol id="i-grip" viewBox="0 0 20 20" {...PIENO}>
          <circle cx="7.6" cy="5.4" r="1.25" />
          <circle cx="12.4" cy="5.4" r="1.25" />
          <circle cx="7.6" cy="10" r="1.25" />
          <circle cx="12.4" cy="10" r="1.25" />
          <circle cx="7.6" cy="14.6" r="1.25" />
          <circle cx="12.4" cy="14.6" r="1.25" />
        </symbol>
        <symbol id="i-chev-d" viewBox="0 0 20 20" {...TRATTO}>
          <path d="m5.6 7.8 4.4 4.4 4.4-4.4" />
        </symbol>
        <symbol id="i-chev-u" viewBox="0 0 20 20" {...TRATTO}>
          <path d="m5.6 12.2 4.4-4.4 4.4 4.4" />
        </symbol>
        <symbol id="i-chev-l" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M12.2 5.6 7.8 10l4.4 4.4" />
        </symbol>
        <symbol id="i-chev-r" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M7.8 5.6 12.2 10l-4.4 4.4" />
        </symbol>
        <symbol id="i-star" viewBox="0 0 20 20" {...PIENO}>
          <path d="m10 3.4 2.06 4.18 4.61.67-3.34 3.25.79 4.59L10 13.92l-4.12 2.17.79-4.59L3.33 8.25l4.61-.67L10 3.4Z" />
        </symbol>
        <symbol
          id="i-star-o"
          viewBox="0 0 20 20"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          strokeLinejoin="round"
        >
          <path d="m10 3.9 1.87 3.79 4.18.61-3.02 2.95.71 4.16L10 13.44l-3.74 1.97.71-4.16L3.95 8.3l4.18-.61L10 3.9Z" />
        </symbol>
        <symbol id="i-skin" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M10 3.4s5 5.3 5 8.1a5 5 0 0 1-10 0c0-2.8 5-8.1 5-8.1Z" />
        </symbol>
        <symbol id="i-cloud" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M6.1 15.4a3.3 3.3 0 0 1-.5-6.56 4.4 4.4 0 0 1 8.34-1.3 3.5 3.5 0 0 1 .36 6.96" />
          <path d="M10 15.9V9.4" />
          <path d="m7.6 11.4 2.4-2.4 2.4 2.4" />
        </symbol>
        <symbol id="i-import" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M10 3.6v8" />
          <path d="m6.6 8.2 3.4 3.4 3.4-3.4" />
          <path d="M3.8 13.4v2a1 1 0 0 0 1 1h10.4a1 1 0 0 0 1-1v-2" />
        </symbol>
        <symbol id="i-scan" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M16.3 8.6A6.5 6.5 0 1 0 16 12.4" />
          <path d="M16.6 4.4v4.2h-4.2" />
        </symbol>
        <symbol
          id="i-check"
          viewBox="0 0 20 20"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.8}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="m4.6 10.4 3.6 3.6 7.2-7.8" />
        </symbol>
        <symbol id="i-alert" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M10 4.2 3.6 15.4h12.8L10 4.2Z" />
          <path d="M10 8.6v3.1M10 13.7v.1" />
        </symbol>
        <symbol id="i-expand" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M11.8 4.4h3.8v3.8M8.2 15.6H4.4v-3.8" />
          <path d="M15.6 4.4 11 9M4.4 15.6 9 11" />
        </symbol>
        <symbol id="i-sort" viewBox="0 0 20 20" {...TRATTO}>
          <path d="M4.4 5.8h11.2M4.4 10h7M4.4 14.2h4" />
        </symbol>
        <symbol
          id="i-eq"
          viewBox="0 0 20 20"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.6}
          strokeLinecap="round"
        >
          <path d="M6 15.6V8.4M10 15.6V4.4M14 15.6v-5.2" />
        </symbol>
        <symbol
          id="i-text"
          viewBox="0 0 20 20"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.6}
          strokeLinecap="round"
        >
          <path d="M4 5.4h12M4 9h9M4 12.6h11M4 16.2h6" />
        </symbol>
        {/*
         * Il marchio: due archi che non si chiudono, uno del colore del testo e
         * uno del ciano della tavolozza. Il secondo è l'unico colore scritto in
         * un simbolo, e viene dal token, non da un letterale.
         */}
        <symbol id="i-mark" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={2.6}>
          <path d="M10 2.4a7.6 7.6 0 0 1 0 15.2" stroke="currentColor" />
          <path d="M10 17.6a7.6 7.6 0 0 1 0-15.2" stroke="var(--skin-color-ciano)" />
        </symbol>
      </defs>
    </svg>
  );
}

/** Cosa serve per disegnare un'icona. */
type Proprieta = {
  /** Quale. */
  nome: NomeIcona;
  /** Quanti pixel di lato. Il disegno usa 13, 15, 16, 17, 19 e 20. */
  dim?: number;
  /**
   * Cosa legge un lettore di schermo.
   *
   * Assente per difetto: la stragrande maggioranza delle icone di questa
   * interfaccia sta **dentro** un bottone che ha già la sua etichetta, e
   * ripeterla la farebbe leggere due volte. Si passa solo quando l'icona è
   * l'unico contenuto e non c'è un `aria-label` intorno.
   */
  titolo?: string;
};

/**
 * Un'icona.
 *
 * `focusable="false"` non è superfluo: Internet Explorer non c'entra, ma alcuni
 * motori mettono comunque gli `<svg>` nell'ordine di tabulazione, e un'icona
 * dentro un bottone raddoppierebbe le fermate del tasto Tab.
 */
export function Icona({ nome, dim = 17, titolo }: Proprieta) {
  return (
    <svg
      width={dim}
      height={dim}
      viewBox="0 0 20 20"
      className="icona"
      focusable="false"
      {...(titolo === undefined ? { "aria-hidden": true } : { role: "img", "aria-label": titolo })}
    >
      <use href={`#${nome}`} />
    </svg>
  );
}
