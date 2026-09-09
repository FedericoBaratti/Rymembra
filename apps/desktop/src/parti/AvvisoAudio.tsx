/**
 * Quel che si dice dell'uscita audio quando c'è qualcosa da dire.
 *
 * # Perché un componente e non due blocchi uguali
 *
 * Perché erano già due: la fascia flottante in `App` e quella in cima alla
 * terza colonna disegnavano lo stesso identico ripiego — `tSe` sul codice della
 * causa, la frase italiana del nucleo se la chiave non c'è, il tasto legato a
 * `riapribile` — in due copie che si sono scritte a mesi di distanza. Servirne
 * una terza per il messaggio del dispositivo cambiato voleva dire tre posti in
 * cui aggiornare la stessa frase, cioè due posti in cui dimenticarsene.
 *
 * # I due avvisi, e perché non sono lo stesso
 *
 * - **Non c'è audio** (`role="alert"`): non si sente niente e non si sentirà
 *   finché qualcosa non cambia. È rosso, resta, e ha un tasto.
 * - **Il suono si è spostato** (`role="status"`): si sente, ma da un'altra
 *   parte. Non è un guasto — è la riapertura automatica che ha funzionato — e
 *   dirlo con la stessa fascia rossa insegnerebbe a ignorarla.
 *
 * Il secondo esiste perché la riapertura è automatica. Prima il gesto di
 * premere «Riapri» diceva da sé che qualcosa era cambiato; adesso il suono si
 * sposta da solo, e senza una riga che lo dica chi ha staccato le cuffie sente
 * la musica uscire dagli altoparlanti senza capire perché. È la stessa
 * informazione di prima, data invece che chiesta.
 */
import { useEffect, useRef, useState } from "react";

import { ipc, type StatoRiproduzione } from "../ipc";
import { Icona } from "./Icone";
import { t, tSe } from "../lingue";

/**
 * Quanto resta a schermo l'avviso del dispositivo cambiato, in millisecondi.
 *
 * Sei secondi: abbastanza per leggerlo mentre si guarda altrove — chi ha appena
 * staccato le cuffie sta guardando il cavo, non lo schermo — e poco abbastanza
 * da non restare lì a fare da sfondo. Non è un guasto: non deve chiedere di
 * essere chiuso.
 */
const DURATA_SPOSTATO = 6_000;

/**
 * Il nome dell'uscita su cui il suono si è appena spostato, finché va detto.
 *
 * `null` quasi sempre. Diventa un nome solo sul **passaggio** fra due uscite
 * diverse, e torna `null` da sé dopo [`DURATA_SPOSTATO`].
 *
 * # Perché si ricava qui e non arriva dal nucleo
 *
 * Perché il nucleo dice già dove sta suonando, a ogni stato: «è cambiato» è la
 * differenza fra due di quei valori, e calcolarla qui costa un `useRef` mentre
 * mandarla di là costerebbe un evento in più che dice quel che lo stato dice
 * già — cioè due sorgenti per lo stesso fatto, che è il difetto che questo
 * modulo evita ovunque.
 *
 * Il primo valore non è un passaggio: all'avvio si passa da «non lo so» a
 * «Altoparlanti», e annunciarlo vorrebbe dire un avviso a ogni apertura della
 * finestra.
 */
function useSpostato(uscita: string | null): string | null {
  const precedente = useRef<string | null>(null);
  const [spostato, setSpostato] = useState<string | null>(null);

  useEffect(() => {
    const prima = precedente.current;
    precedente.current = uscita;
    // Il primo valore, o il ritorno a «nessuna uscita»: né l'uno né l'altro
    // sono uno spostamento da raccontare.
    if (uscita === null || prima === null || prima === uscita) return;
    setSpostato(uscita);
    const conto = window.setTimeout(() => setSpostato(null), DURATA_SPOSTATO);
    return () => window.clearTimeout(conto);
  }, [uscita]);

  return spostato;
}

/** Dove sta la fascia, che qui vuol dire soltanto con che vestito. */
export type Dove = "fascia" | "colonna";

export function AvvisoAudio({
  stato,
  dove,
  onErrore,
}: {
  stato: StatoRiproduzione;
  dove: Dove;
  /** Un «Riapri» che fallisce va detto: è l'ultimo tasto rimasto. */
  onErrore: (e: unknown) => void;
}) {
  const spostato = useSpostato(stato.uscita);

  if (stato.audio !== null) {
    const guasto = stato.audio;
    /* La causa tradotta dal codice, con la frase del nucleo come ripiego.
       `tSe` perché il codice non lo può verificare TypeScript: arriva da
       `aether-play`, che di lingue non ne conosce. Prima si stampava la frase
       e basta, ed era italiana accanto a un'etichetta inglese. */
    const perche =
      guasto.causaCodice === null
        ? guasto.causa
        : tSe(`audio.lost.cause.${guasto.causaCodice}`, guasto.causa);
    const tasto = guasto.riapribile && (
      <button
        type="button"
        className="bottone minuto btn-ghost"
        /* Il brano si ritrova dov'era e riparte se stava andando: il tasto non
           è più l'unica strada — il nucleo riapre da sé quando l'elenco delle
           uscite cambia — ma resta per i guasti che dall'elenco non si vedono. */
        title={t("audio.lost.reopen.title")}
        onClick={() => {
          ipc.riapriAudio().catch(onErrore);
        }}
      >
        {dove === "colonna" && <Icona nome="i-repeat" dim={14} />}
        {t("audio.lost.reopen")}
      </button>
    );

    if (dove === "colonna") {
      return (
        <div className="audio-perso" role="alert">
          <div className="cosa">
            <strong>{t("audio.lost.what")}</strong> {perche}.
          </div>
          {tasto}
        </div>
      );
    }
    return (
      <div className="errore audio-perso toast-card" role="alert">
        <Icona nome="i-alert" dim={16} />
        <span>
          <strong>{t("audio.lost.what")}</strong> {perche}.
        </span>
        {tasto}
      </div>
    );
  }

  if (spostato === null) return null;

  // Lo spostamento. Nella colonna riusa il vestito della fascia — stessa
  // larghezza, stessi margini — con la tinta dell'accento al posto di quella
  // dell'allarme: dice che è successo qualcosa, non che c'è qualcosa che non va.
  const testo = (
    <>
      <strong>{t("audio.moved.what")}</strong>{" "}
      {t("audio.moved.where", { nome: spostato })}
    </>
  );
  if (dove === "colonna") {
    return (
      <div className="audio-perso audio-spostato" role="status">
        <div className="cosa">{testo}</div>
      </div>
    );
  }
  return (
    <div className="notizia audio-spostato toast-card" role="status">
      <Icona nome="i-check" dim={16} />
      <span>{testo}</span>
    </div>
  );
}
