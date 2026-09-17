/**
 * Il volume sotto il dito: il cursore che si muove a ogni pixel, e il nucleo
 * che lo sente venti volte al secondo.
 *
 * # Perché
 *
 * Un `<input type="range">` manda un `change` per ogni pixel: trascinarlo da un
 * capo all'altro erano centinaia di comandi `volume`, e ognuno tornava come un
 * evento che ridisegnava l'applicazione intera. L'orecchio non distingue venti
 * passi al secondo da cento.
 *
 * Il valore mostrato sta qui finché il nucleo non lo raggiunge, così il cursore
 * non torna indietro di un passo a ogni risposta in ritardo; quando il nucleo è
 * arrivato — e non c'è altro in volo — si torna a leggere il suo.
 *
 * Due cursori lo usano, quello della fascia del giudizio e quello della barra
 * del lettore: una copia sola, perché due strozzature scritte a mano sono due
 * strozzature che divergono al primo difetto.
 */
import { useEffect, useRef, useState } from "react";

import { ipc } from "./ipc";

/** Ogni quanto, al massimo, il cursore parla col nucleo. */
const PASSO_VOLUME_MS = 50;

/**
 * Il valore da mostrare, e il gesto che lo cambia.
 *
 * Cambiare il volume dal cursore toglie il silenziamento: chi trascina la
 * manopola sta chiedendo di sentire.
 */
export function useVolumeSottoIlDito(
  delNucleo: number,
  onErrore: (e: unknown) => void,
): { volume: number; sposta: (livello: number) => void } {
  const [trascinato, setTrascinato] = useState<number | null>(null);
  const ultimoInvio = useRef(0);
  const inAttesa = useRef<number | undefined>(undefined);
  const daMandare = useRef<number | null>(null);
  /** I comandi mandati che non hanno ancora risposto. */
  const inVolo = useRef(0);
  /** Quante risposte sono tornate: fa rigirare l'effetto qui sotto. */
  const [risposte, setRisposte] = useState(0);
  /** Il volume del nucleo com'era all'ultima risposta. */
  const nucleoAllaRisposta = useRef(delNucleo);
  const nucleoOra = useRef(delNucleo);
  useEffect(() => {
    nucleoOra.current = delNucleo;
  });

  useEffect(() => () => window.clearTimeout(inAttesa.current), []);

  /* Quando si torna a leggere il volume del nucleo.

     Solo a mani vuote — niente in attesa, niente in volo — e allora in due
     casi: il nucleo dice il valore trascinato, o ha detto qualcosa **dopo**
     l'ultima risposta, cioè un volume che non viene da qui (la tastiera, il
     vassoio, un altro cursore).

     Prima c'era solo il primo caso, e l'effetto dipendeva da due riferimenti che
     non lo fanno rigirare. Se l'eco del valore arrivava prima che il valore
     partisse — un trascinamento che torna sul punto già mandato, e l'ultimo
     invio che parte dal timer — l'effetto non girava più: il cursore restava
     fermo sul valore trascinato, e il volume cambiato dalla tastiera non si
     vedeva fino al trascinamento dopo. */
  useEffect(() => {
    if (
      trascinato === null ||
      daMandare.current !== null ||
      inAttesa.current !== undefined ||
      inVolo.current > 0
    )
      return;
    if (
      Math.abs(delNucleo - trascinato) < 0.005 ||
      delNucleo !== nucleoAllaRisposta.current
    )
      setTrascinato(null);
  }, [delNucleo, trascinato, risposte]);

  const sposta = (livello: number) => {
    setTrascinato(livello);
    daMandare.current = livello;
    const invia = () => {
      inAttesa.current = undefined;
      const valore = daMandare.current;
      if (valore === null) return;
      daMandare.current = null;
      ultimoInvio.current = performance.now();
      inVolo.current += 1;
      ipc
        .volume(valore, false)
        .then(() => {
          inVolo.current -= 1;
          nucleoAllaRisposta.current = nucleoOra.current;
          setRisposte((n) => n + 1);
        })
        .catch((e: unknown) => {
          inVolo.current -= 1;
          // Il nucleo non l'ha preso: il cursore torna a dire il volume vero.
          setTrascinato(null);
          onErrore(e);
        });
    };
    if (inAttesa.current !== undefined) return;
    const passato = performance.now() - ultimoInvio.current;
    if (passato >= PASSO_VOLUME_MS) invia();
    else inAttesa.current = window.setTimeout(invia, PASSO_VOLUME_MS - passato);
  };

  return { volume: trascinato ?? delNucleo, sposta };
}
