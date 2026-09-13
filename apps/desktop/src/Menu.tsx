/**
 * Il menù che si apre col tasto destro.
 *
 * Esiste perché «riproduci dopo» e «accoda» non hanno un posto sulla riga:
 * l'elenco ha già sei colonne, e due tasti in più per ogni brano sarebbero due
 * tasti da guardare mille volte per usarli una. Da qui passerà anche «aggiungi
 * a playlist», che è la stessa forma di comando.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * Quanto il menù sta lontano dal bordo della finestra.
 *
 * Otto pixel: lo stesso respiro che `--spazio-2` dà dentro il menù, perché un
 * menù incollato al bordo si legge come tagliato anche quando è intero.
 */
const MARGINE = 8;

/** Una voce del menù. */
export interface Voce {
  etichetta: string;
  azione: () => void;
}

/** Dove si è aperto e su cosa. */
export interface Apertura {
  x: number;
  y: number;
  voci: Voce[];
}

export function Menu({
  apertura,
  onChiudi,
}: {
  apertura: Apertura;
  onChiudi: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const chiAveva = useRef<HTMLElement | null>(null);
  const [posto, setPosto] = useState<{ left: number; top: number } | null>(null);

  /**
   * Dove sta il menù lo decide la sua misura, non un numero scritto qui.
   *
   * Prima lo decidevano due numeri a mano: `220px` di larghezza e `34px` per
   * voce, dentro un `min()` nel foglio di stile. Erano giusti per il menù di
   * allora e per la densità di allora, e sbagliati appena una cosa sola cambiava:
   * un'etichetta lunga — «Aggiungi alla playlist…» in tedesco — supera i
   * duecentoventi e il menù sborda a destra; `data-density='spacious'`, o un
   * corpo del testo più grande, alza le voci sopra i trentaquattro e l'ultima
   * finisce sotto il bordo inferiore. Il menù non ha una larghezza dichiarata
   * (`min-width: 200px` e poi quel che serve) proprio perché non si vuole
   * troncare un'etichetta: il numero, quindi, non si poteva sapere.
   *
   * Si misura. `useLayoutEffect` gira **dopo** il disegno e **prima** della
   * pittura: lo stato che ne esce rientra nello stesso fotogramma, quindi il
   * fotogramma nel posto sbagliato che il vecchio commento temeva non esiste.
   * Nel primo passaggio il menù sta a zero e resta invisibile, e non è un
   * dettaglio di prudenza: un elemento `fixed` ancorato a destra si **stringe**
   * nello spazio che gli resta, e misurarlo là dove sborda darebbe una larghezza
   * più piccola del vero e un'etichetta andata a capo. A zero ha davanti tutta la
   * finestra, quindi quella che si legge è la sua misura naturale.
   *
   * Il ribaltamento resta il comportamento di prima — il menù si tira dentro dal
   * bordo invece di andare sopra il puntatore — e in più non esce mai dall'alto
   * né da sinistra, che `min()` da solo non garantiva.
   */
  useLayoutEffect(() => {
    const nodo = menu.current;
    if (nodo === null) return;
    const { width, height } = nodo.getBoundingClientRect();
    const dentro = (voluto: number, misura: number, quanto: number) =>
      Math.max(MARGINE, Math.min(voluto, quanto - misura - MARGINE));
    setPosto({
      left: dentro(apertura.x, width, window.innerWidth),
      top: dentro(apertura.y, height, window.innerHeight),
    });
  }, [apertura]);

  /**
   * Escape chiude, le frecce percorrono.
   *
   * Un menù senza frecce è un menù solo per il mouse: `role="menu"` promette a
   * chi legge lo schermo che ArrowDown scende alla voce dopo, e senza questo la
   * promessa è falsa — resterebbe solo il Tab, che qui non è quel che si aspetta
   * chi arriva da un menù del sistema. Il giro si avvolge agli estremi, così
   * dall'ultima voce si torna alla prima senza risalirle tutte; Home ed End
   * saltano ai capi. Se il fuoco è ancora fuori dal menù, ArrowDown parte dalla
   * prima voce e ArrowUp dall'ultima.
   */
  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onChiudi();
        return;
      }
      const voci = Array.from(menu.current?.querySelectorAll("button") ?? []);
      if (voci.length === 0) return;
      const quale = voci.findIndex((voce) => voce === document.activeElement);
      let prossima: number;
      if (e.key === "ArrowDown")
        prossima = quale + 1 >= voci.length ? 0 : quale + 1;
      else if (e.key === "ArrowUp")
        prossima = quale <= 0 ? voci.length - 1 : quale - 1;
      else if (e.key === "Home") prossima = 0;
      else if (e.key === "End") prossima = voci.length - 1;
      else return;
      e.preventDefault();
      voci[prossima]?.focus();
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  // Il fuoco entra nel menù appena si apre: chi naviga da tastiera deve poterlo
  // percorrere senza prima ritrovare con Tab il punto in cui si è aperto. Alla
  // chiusura si prova a rimetterlo dov'era, e il ripristino vale quando chi ha
  // aperto è un elemento focalizzabile ancora montato: i tasti di
  // `parti/Navigazione.tsx`, i comandi della barra. Sul percorso principale non
  // lo è: il menù nasce dal tasto destro su `<div>` che non prendono il fuoco
  // (la riga di `App.tsx`, le schede album), lì `document.activeElement` è il
  // `<body>` e rimettercelo non fa niente. La guardia lo dice invece di
  // fingerlo: è un no-op scritto, non un cambio di comportamento — il fuoco
  // resta dove sarebbe caduto comunque, cioè sul `<body>`, come prima.
  //
  // Un «ancoraggio» esplicito passato da chi apre coprirebbe anche quel caso,
  // ma cambierebbe le prop di `Menu` e tutti i suoi chiamanti: fuori dal
  // perimetro di un raffinamento a comportamento invariato. È il candidato
  // successivo.
  //
  // Chi aveva il fuoco va letto prima di portarlo dentro il menù.
  useEffect(() => {
    chiAveva.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    menu.current?.querySelector("button")?.focus();
    return () => {
      const chi = chiAveva.current;
      if (chi && chi !== document.body && chi.isConnected) chi.focus();
    };
  }, []);

  return (
    <>
      {/* Il velo prende il clic di chiusura, compreso il destro: senza, un
          secondo tasto destro altrove aprirebbe un menù col primo ancora su. */}
      <div
        className="velo"
        onClick={onChiudi}
        onContextMenu={(e) => {
          e.preventDefault();
          onChiudi();
        }}
      />
      <div
        ref={menu}
        className="menu menu-pop"
        role="menu"
        /* Il menù si ancora in alto a sinistra del puntatore e si tira dentro dal
           bordo quando sborderebbe. Il primo passaggio è la misura: vedi la nota
           dell'effetto qui sopra. */
        style={
          posto === null
            ? { left: 0, top: 0, visibility: "hidden" }
            : { left: posto.left, top: posto.top }
        }
      >
        {apertura.voci.map((voce) => (
          <button
            key={voce.etichetta}
            type="button"
            role="menuitem"
            onClick={() => {
              voce.azione();
              onChiudi();
            }}
          >
            {voce.etichetta}
          </button>
        ))}
      </div>
    </>
  );
}
