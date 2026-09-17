/**
 * I tre comandi della finestra, e la fascia da cui la si trascina.
 *
 * # Perché sta fuori dall'applicazione
 *
 * Si monta in `main.tsx` accanto ad `App`, non dentro. Non è una scelta di
 * eleganza: `App` **si sostituisce** con lo Studio quando si apre l'editor delle
 * skin — è un `return` anticipato, non un pannello — e una finestra che perde i
 * suoi tre bottoni entrando in una schermata sarebbe un guasto. Fuori, valgono
 * per ogni schermata che l'applicazione possa mostrare, compresa quella d'errore.
 *
 * Per la stessa ragione non passa dallo scafale delle skin: una skin che potesse
 * spostare, nascondere o ridipingere il tasto di chiusura potrebbe rendere la
 * finestra impossibile da chiudere, e le skin si installano da un file.
 *
 * # La fascia non è un elemento
 *
 * Trascinare la finestra si fa premendo in alto e muovendo, e la strada ovvia —
 * un riquadro trasparente largo quanto la finestra sopra tutto il resto —
 * mangerebbe ogni clic dei primi sessanta pixel: il tasto che richiude la
 * navigazione, i due della terza colonna, la ricerca quando va a capo.
 *
 * Quindi non c'è nessun riquadro: c'è un ascoltatore su `document` che guarda
 * dove è caduto il `mousedown`. Sopra la riga della fascia e **non** dentro
 * qualcosa su cui si clicca, la finestra si trascina; altrove non è successo
 * niente. Il vantaggio non è solo di non rubare clic: si afferra anche il titolo
 * della pagina e la scritta «In riproduzione», che un riquadro trasparente
 * avrebbe coperto ma che sono, a guardarli, esattamente la barra del titolo.
 *
 * L'altezza della fascia non è scritta qui. La misura sta nel foglio
 * (`--barra-titolo-h`) e questo file la **legge** dal riquadro dei bottoni, che è
 * alto quanto lei: è la stessa regola per cui nessun componente di questa
 * cartella calcola geometria.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { ipc } from "../ipc";
import { Icona } from "./Icone";
import { t, useLingua } from "../lingue";

/**
 * Quel che, nella fascia, non è barra del titolo.
 *
 * Tutto ciò su cui si clicca o si scrive, più il velo di una finestrella: il
 * velo *è* un bersaglio: prende il clic per chiudere, e trascinarci la finestra
 * avrebbe chiuso la finestrella sotto le dita a fine trascinamento.
 */
const NON_SI_AFFERRA = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  "label",
  "[role='button']",
  "[role='tab']",
  "[contenteditable]",
  ".velo",
].join(",");

export function BarraTitolo() {
  // Iscritta alla lingua anche se il valore non serve: montata fuori da `App`,
  // nessun disegno di `App` la raggiunge, e i tre nomi dei bottoni restavano
  // nella lingua dell'avvio — «Riduci a icona» con l'interfaccia in inglese.
  useLingua();
  const riquadro = useRef<HTMLDivElement>(null);
  const [ingrandita, setIngrandita] = useState(false);

  /**
   * Se è ingrandita adesso.
   *
   * Non basta guardare i nostri bottoni: si ingrandisce anche col doppio clic
   * sulla fascia, con `Win`+`↑`, e affiancando la finestra a un bordo dello
   * schermo. Tutte e quattro le strade ridimensionano la pagina, e questa è la
   * ragione per cui la domanda si rifà a ogni `resize` invece che dopo ogni
   * clic.
   */
  const chiediStato = useCallback(() => {
    ipc
      .finestraIngrandita()
      .then(setIngrandita)
      .catch(() => {
        // Una finestra che non sa dire com'è non è un errore da mostrare a chi
        // ascolta musica: l'icona resta quella di prima.
      });
  }, []);

  useEffect(() => {
    chiediStato();
    // Non a ogni fotogramma: trascinare un bordo produce decine di `resize` al
    // secondo, e ognuno sarebbe un giro di IPC sul thread principale per una
    // risposta che durante il trascinamento è sempre «no». Si chiede quando il
    // ridimensionamento si è fermato.
    let attesa: number | undefined;
    const dopo = () => {
      window.clearTimeout(attesa);
      attesa = window.setTimeout(chiediStato, 120);
    };
    window.addEventListener("resize", dopo);
    return () => {
      window.clearTimeout(attesa);
      window.removeEventListener("resize", dopo);
    };
  }, [chiediStato]);

  useEffect(() => {
    const premuto = (e: MouseEvent) => {
      // Solo il tasto sinistro: col destro Windows vuole il menù di sistema, e
      // `Alt`+`Spazio` continua ad aprirlo perché non lo intercetta nessuno.
      if (e.button !== 0) return;
      const fascia = riquadro.current?.getBoundingClientRect().bottom ?? 0;
      if (e.clientY > fascia) return;
      const dove = e.target as Element | null;
      if (!dove || dove.closest(NON_SI_AFFERRA)) return;

      // Il secondo clic di un doppio: ingrandisce o rimette com'era, come fa la
      // fascia di qualunque finestra. Il primo ha già avviato il trascinamento,
      // che senza movimento non ha spostato niente.
      if (e.detail === 2) {
        ipc.finestraIngrandisci().then(setIngrandita).catch(chiediStato);
        return;
      }
      ipc.finestraTrascina().catch(() => {
        // Il trascinamento è del sistema operativo: se non parte, la finestra
        // resta dov'è e non c'è niente da dire.
      });
    };
    document.addEventListener("mousedown", premuto);
    return () => document.removeEventListener("mousedown", premuto);
  }, [chiediStato]);

  return (
    <div className="controlli-finestra" ref={riquadro}>
      <button
        type="button"
        className="controllo"
        aria-label={t("window.minimize")}
        onClick={() => void ipc.finestraRiduci().catch(() => {})}
      >
        <Icona nome="i-win-min" dim={16} />
      </button>
      <button
        type="button"
        className="controllo"
        aria-label={ingrandita ? t("window.restore") : t("window.maximize")}
        onClick={() => {
          ipc.finestraIngrandisci().then(setIngrandita).catch(chiediStato);
        }}
      >
        <Icona nome={ingrandita ? "i-win-restore" : "i-win-max"} dim={16} />
      </button>
      <button
        type="button"
        className="controllo chiudi"
        aria-label={t("window.close")}
        onClick={() => void ipc.finestraChiudi().catch(() => {})}
      >
        <Icona nome="i-x" dim={16} />
      </button>
    </div>
  );
}
