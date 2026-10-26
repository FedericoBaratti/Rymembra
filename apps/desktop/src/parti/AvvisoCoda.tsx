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
 */
import { useEffect, useState } from "react";

import { gestoPermesso } from "../insieme";
import { ipc } from "../ipc";
import { useAscolto } from "../pagine";
import { useLettore } from "../riproduzione";
import { Annuncio } from "./Annuncio";
import { Icona } from "./Icone";
import { brani_ } from "../formato";
import { t } from "../lingue";

/**
 * Quanto resta a schermo, in millisecondi.
 *
 * Dieci secondi, più dei sei dell'uscita spostata in `AvvisoAudio`: quello si
 * legge e basta, questo chiede un gesto, e chi si accorge di aver perso la coda
 * se ne accorge quando la musica cambia — un paio di secondi dopo il clic.
 */
const DURATA = 10_000;

/** Quel che arriva con `coda:sostituita`. */
interface CodaSostituita {
  numero: number;
  brani: number;
  svuotata: boolean;
}

export function AvvisoCoda({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [avviso, setAvviso] = useState<CodaSostituita | null>(null);
  /*
   * In una sessione di «Ascolta insieme» l'«Annulla» non c'è, per nessuno:
   * rimetterebbe una coda di prima della sessione, fatta di brani che gli
   * altri non hanno, e il nucleo lo rifiuta (`strada.rs`). Non si spegne il
   * tasto: si tace l'avviso intero, perché «Coda sostituita — Annulla» senza
   * un annulla è una notizia su un gesto che si è appena visto fare.
   */
  const ripristinabile = useLettore((s) => gestoPermesso(s.insieme, "ripristina"));

  useAscolto<CodaSostituita>("coda:sostituita", (carico) => {
    if (ripristinabile) setAvviso(carico);
  });

  // Una sessione che comincia con l'avviso a schermo lo chiude: l'«Annulla»
  // di prima, premuto adesso, chiederebbe la stessa cosa che la sessione vieta.
  useEffect(() => {
    if (!ripristinabile) setAvviso(null);
  }, [ripristinabile]);

  // Un avviso nuovo riparte da dieci secondi: ogni evento è un oggetto nuovo,
  // quindi il conto si rifà anche quando uno ne sostituisce un altro a schermo.
  useEffect(() => {
    if (avviso === null) return;
    const conto = window.setTimeout(() => setAvviso(null), DURATA);
    return () => window.clearTimeout(conto);
  }, [avviso]);

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
        <div className="notizia toast-card">
          <Icona nome="i-check" dim={16} />
          <span>{frase}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            title={t("queue.undo.aria")}
            onClick={() => {
              const numero = avviso.numero;
              setAvviso(null);
              ipc.codaRipristina(numero).catch(onErrore);
            }}
          >
            {t("queue.undo")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("common.close")}
            onClick={() => setAvviso(null)}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
    </>
  );
}
