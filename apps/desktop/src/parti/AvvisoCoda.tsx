/**
 * «Coda sostituita — Annulla».
 *
 * # Perché un annulla e non una conferma
 *
 * Perché il gesto che sostituisce la coda è quasi sempre voluto, e una domanda
 * a ogni doppio clic insegnerebbe a rispondere «sì» senza leggere — cioè a
 * perdere la coda comunque, con un clic in più. Il caso che ha fatto nascere
 * questo avviso è l'altro: un Invio su una cartella che ha buttato via una
 * coda da duecento brani, senza nessuna strada per riaverla.
 *
 * Quando l'avviso vale lo decide il nucleo (`riproduzione::coda`): una coda
 * costruita a mano, una coda lunga, o «Svuota». Una coda nata da un album che
 * lascia il posto a un altro album non manda niente, e qui non arriva niente.
 *
 * # Perché il numero
 *
 * L'evento porta il numero della sostituzione, e «Annulla» lo ripassa. Se nei
 * secondi in cui l'avviso è a schermo la coda è stata sostituita di nuovo, il
 * nucleo risponde `false` e non tocca niente: rimettere la coda di prima del
 * gesto **successivo** sarebbe un annulla che non annulla quel che dice.
 *
 * # Perché lo stato sta fuori dal componente
 *
 * Il riquadro si disegna in due posti — nella pagina e sopra «In riproduzione»
 * — e `Ctrl+Z` fa l'ultimo «Annulla» dovunque sia il fuoco. Con uno stato per
 * copia il gesto ne avrebbe chiusa una e lasciata l'altra col suo «Annulla»
 * ancora premibile, cioè un secondo ripristino dello stesso gesto. Lo stato, il
 * conto e l'offerta a `Ctrl+Z` stanno una volta, in `ANNULLO`; le copie lo
 * leggono. Il conto si ferma col fuoco o il puntatore sul riquadro: vedi
 * `annullaATempo.ts`.
 */
import { useEffect } from "react";

import { annullaATempo } from "../annullaATempo";
import { useArchivio } from "../archivio";
import { gestoPermesso } from "../insieme";
import { ipc } from "../ipc";
import { useAscolto } from "../pagine";
import { useLettore } from "../riproduzione";
import { Annuncio } from "./Annuncio";
import { Icona } from "./Icone";
import { brani_ } from "../formato";
import { t } from "../lingue";

/** Quel che arriva con `coda:sostituita`. */
interface CodaSostituita {
  numero: number;
  brani: number;
  svuotata: boolean;
}

/** Chi dice un ripristino che non riesce: l'`onErrore` dell'ultima copia disegnata. */
let segnala: (e: unknown) => void = () => undefined;

/**
 * L'«Annulla» della coda, uno per tutte e due le copie. Il numero torna al
 * nucleo: vedi «Perché il numero» qui sopra.
 */
const ANNULLO = annullaATempo<CodaSostituita>((avviso) => {
  ipc.codaRipristina(avviso.numero).catch((e: unknown) => segnala(e));
});

export function AvvisoCoda({ onErrore }: { onErrore: (e: unknown) => void }) {
  useEffect(() => {
    segnala = onErrore;
  }, [onErrore]);
  const avviso = useArchivio(ANNULLO.avviso);
  /*
   * In una sessione di «Ascolta insieme» l'«Annulla» non c'è, per nessuno:
   * rimetterebbe una coda di prima della sessione, fatta di brani che gli
   * altri non hanno, e il nucleo lo rifiuta (`strada.rs`). Non si spegne il
   * tasto: si tace l'avviso intero, perché «Coda sostituita — Annulla» senza
   * un annulla è una notizia su un gesto che si è appena visto fare.
   */
  const ripristinabile = useLettore((s) => gestoPermesso(s.insieme, "ripristina"));

  // Tutte e due le copie ascoltano, e l'avviso è uno: la seconda trova già
  // a schermo lo stesso numero, e non ricomincia il conto.
  useAscolto<CodaSostituita>("coda:sostituita", (carico) => {
    if (ripristinabile && ANNULLO.avviso.leggi()?.numero !== carico.numero) {
      ANNULLO.mostra(carico);
    }
  });

  // Una sessione che comincia con l'avviso a schermo lo chiude: l'«Annulla»
  // di prima, premuto adesso, chiederebbe la stessa cosa che la sessione vieta.
  useEffect(() => {
    if (!ripristinabile) ANNULLO.mostra(null);
  }, [ripristinabile]);

  // «1 brano» o «12 brani»: la forma la sceglie il motore delle lingue.
  const frase =
    avviso === null
      ? null
      : avviso.svuotata
        ? t("queue.undo.cleared", { brani: brani_(avviso.brani) })
        : t("queue.undo.replaced", { brani: brani_(avviso.brani) });

  // La regione prima del riquadro, e montata anche senza avviso: è lei a
  // parlare, perché il riquadro nasce già pieno. Vedi `Annuncio.tsx`.
  return (
    <>
      <Annuncio testo={frase} />
      {avviso !== null && (
        <div className="notizia toast-card" {...ANNULLO.presa}>
          <Icona nome="i-check" dim={16} />
          <span>{frase}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            title={t("queue.undo.aria")}
            onClick={ANNULLO.annulla}
          >
            {t("queue.undo")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("common.close")}
            onClick={() => ANNULLO.mostra(null)}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
    </>
  );
}
