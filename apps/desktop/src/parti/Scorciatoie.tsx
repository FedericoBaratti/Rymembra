/**
 * Riassegnare i tasti.
 *
 * # Perché si cattura la pressione invece di scrivere il nome
 *
 * Un campo in cui digitare `Ctrl+Shift+K` è un campo in cui si sbaglia: la
 * stessa combinazione si scrive in sei modi, e nessuno sa come chiamare il
 * tasto fra `Ctrl` e `Alt` sulla propria tastiera. Qui si preme il tasto e si
 * legge quel che è arrivato — la stessa funzione che poi lo riconoscerà,
 * `tastoDi`, quindi non esiste una combinazione che si può registrare e non
 * scattare.
 *
 * # Perché un conflitto si dice invece di rifiutarsi
 *
 * Assegnare Spazio a due comandi è un errore, ma il momento in cui lo si
 * commette è a metà di un lavoro: si sta spostando una scorciatoia da un
 * comando all'altro, e per un istante ce l'hanno tutti e due. Rifiutare la
 * seconda assegnazione obbligherebbe a fare i passi nell'ordine giusto senza
 * dirlo. Si accetta, si dichiara, e chi legge sistema. Nel frattempo
 * l'ascoltatore ha comunque una regola definita — vince il primo dell'elenco —
 * quindi non c'è nessun istante di comportamento imprevedibile.
 */
import { useState } from "react";

import { Icona } from "./Icone";
import {
  COMANDI,
  DI_SERIE,
  type Associazioni,
  type Comando,
  conflitti,
  serieMutabile,
  tastoDi,
  tastoScritto,
} from "../tastiera";

/** I tasti che non si lasciano assegnare, e perché. */
const VIETATI: Record<string, string> = {
  Escape: "Escape chiude quel che è aperto, sempre: è l'uscita di sicurezza.",
  Tab: "Tab sposta il fuoco: serve a chi naviga con la tastiera.",
  Enter: "Invio attiva quel che è a fuoco.",
};

export function Scorciatoie({
  associazioni,
  onCambia,
}: {
  associazioni: Associazioni;
  /** Le nuove associazioni, già intere: le scrive chi le riceve. */
  onCambia: (a: Associazioni) => void;
}) {
  /** Il comando che sta aspettando una pressione, o `null`. */
  const [inAscolto, setInAscolto] = useState<Comando | null>(null);
  /** Perché l'ultima pressione non è stata presa. */
  const [rifiutato, setRifiutato] = useState<string | null>(null);
  const contesi = conflitti(associazioni);

  /** Sostituisce le associazioni di un comando. */
  const scrivi = (comando: Comando, tasti: string[]) => {
    onCambia({ ...associazioni, [comando]: tasti });
  };

  const cattura = (comando: Comando, e: React.KeyboardEvent) => {
    e.preventDefault();
    // Ferma la propagazione, altrimenti la stessa pressione arriverebbe anche
    // all'ascoltatore globale: assegnare lo Spazio metterebbe in pausa.
    e.stopPropagation();
    // Escape non si assegna e non è nemmeno un rifiuto da spiegare: qui vuol
    // dire quel che vuole dire dappertutto, cioè «lascia stare».
    if (e.key === "Escape") {
      setInAscolto(null);
      setRifiutato(null);
      return;
    }
    const tasto = tastoDi(e);
    // `null` = è stato premuto solo un modificatore: la combinazione si sta
    // ancora componendo, e concludere adesso registrerebbe «Ctrl» da solo.
    if (tasto === null) return;
    const vietato = VIETATI[e.key];
    if (vietato !== undefined) {
      setRifiutato(vietato);
      return;
    }
    setRifiutato(null);
    setInAscolto(null);
    // Il tasto **si aggiunge** invece di sostituire: «Cerca» ne ha due di
    // serie, e chi ne aggiunge uno non sta chiedendo di perdere l'altro.
    if (associazioni[comando].includes(tasto)) return;
    scrivi(comando, [...associazioni[comando], tasto]);
  };

  return (
    <>
      <p className="nota">
        Premi <strong>Assegna</strong> e poi il tasto che vuoi. Un comando può
        averne più di uno. <code>Esc</code> non si riassegna: è l&apos;uscita da
        ogni finestrella e da ogni campo, e un&apos;uscita che si può chiudere a
        chiave non è un&apos;uscita.
      </p>

      <ul className="cartelle">
        {COMANDI.map(({ chiave, titolo, spiegazione }) => (
          <li className="cartella scorciatoia" key={chiave}>
            <div className="che-cosa">
              <div className="etichetta">{titolo}</div>
              <div className="spiegazione">{spiegazione}</div>
            </div>
            <div className="tasti">
              {associazioni[chiave].map((tasto) => (
                <button
                  key={tasto}
                  type="button"
                  className="tasto-scorciatoia"
                  title={
                    contesi.has(tasto)
                      ? `${tastoScritto(tasto)} è assegnato anche a un altro comando`
                      : `Togli ${tastoScritto(tasto)}`
                  }
                  data-conteso={contesi.has(tasto) || undefined}
                  onClick={() =>
                    scrivi(
                      chiave,
                      associazioni[chiave].filter((t) => t !== tasto),
                    )
                  }
                >
                  <kbd>{tastoScritto(tasto)}</kbd>
                  <Icona nome="i-x" dim={11} />
                </button>
              ))}
              {associazioni[chiave].length === 0 && (
                <span className="spiegazione">nessuna</span>
              )}
              <button
                type="button"
                className="bottone minuto btn-ghost"
                aria-label={`Assegna un tasto a ${titolo}`}
                data-inascolto={inAscolto === chiave || undefined}
                onKeyDown={(e) => {
                  if (inAscolto === chiave) cattura(chiave, e);
                }}
                onBlur={() => inAscolto === chiave && setInAscolto(null)}
                onClick={() => {
                  setRifiutato(null);
                  setInAscolto(inAscolto === chiave ? null : chiave);
                }}
              >
                {inAscolto === chiave ? "Premi un tasto…" : "Assegna"}
              </button>
            </div>
          </li>
        ))}
      </ul>

      {rifiutato !== null && <p className="nota">{rifiutato}</p>}

      {contesi.size > 0 && (
        <p className="nota">
          <strong>Lo stesso tasto per più comandi</strong>:{" "}
          {[...contesi.entries()]
            .map(
              ([tasto, comandi]) =>
                `${tastoScritto(tasto)} (${comandi
                  .map((c) => COMANDI.find((x) => x.chiave === c)?.titolo ?? c)
                  .join(", ")})`,
            )
            .join("; ")}
          . Finché è così vince il primo di questo elenco — non è casuale, ma
          quasi di sicuro non è quel che volevi.
        </p>
      )}

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={COMANDI.every(
            ({ chiave }) =>
              associazioni[chiave].length === DI_SERIE[chiave].length &&
              associazioni[chiave].every((t, i) => t === DI_SERIE[chiave][i]),
          )}
          onClick={() => onCambia(serieMutabile())}
        >
          Rimetti quelle di serie
        </button>
      </div>
    </>
  );
}
