/**
 * Studio · Documento — la stessa skin, programmata.
 *
 * # L'anteprima non si spegne su un errore
 *
 * Resta all'ultimo stato valido, in pausa e dichiarata tale. Un editor che
 * diventa bianco a metà di una parentesi costringe a scrivere in fretta per
 * paura, e la fretta è il contrario di quel che serve a chi sta accordando dei
 * colori.
 *
 * # Il CSS compilato sta accanto al JSON, in sola lettura
 *
 * È il modo più rapido per capire il contratto: si vede quale proprietà nasce da
 * quale campo. Ed è anche il file che un test del crate desktop confronta col
 * blocco generato di `stile.css` — cioè la cosa che impedisce alla copia di
 * divergere in silenzio.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import { Impaginazione } from "../Impaginazione";
import type { Avviso, Contrasto, Problema, Registro, Validazione } from "../ipc";
import { Icona } from "../parti/Icone";
import { Anteprima } from "./Anteprima";
import { evidenzia, posizione, rigaDi } from "./evidenzia";
import { contestoFinto } from "./finto";

/** Le tre schede del riquadro centrale. */
export type Scheda = "json" | "css" | "diff";

/**
 * L'altezza di una riga dell'editor, in pixel.
 *
 * `12px × 1.72` come in `stile.css`, e la duplicazione è voluta: serve a
 * scorrere fino a una riga, e nessun modo di chiederla al DOM è più corto di
 * così senza misurare un elemento che potrebbe non essere ancora disegnato.
 */
const ALTEZZA_RIGA = 12 * 1.72;

/**
 * Il registro vuoto per l'anteprima ferma di questa vista.
 *
 * Fuori dal componente e non `new Map()` nel JSX: una mappa nuova a ogni
 * disegno è una dipendenza nuova per l'effetto della sonda dentro `Anteprima`,
 * e l'effetto che si rifà azzera lo stato, che ridisegna, che rifà l'effetto.
 * Qui la sonda è spenta e il registro non serve — ma deve essere lo stesso
 * oggetto ogni volta.
 */
const SENZA_REGISTRO = new Map();

/**
 * I due buchi, in questa anteprima.
 *
 * Vuoti apposta: qui l'anteprima serve a vedere che il documento **compila**,
 * non a leggere una pagina. Quel che conta è la cornice — e la cornice viene
 * tutta dall'albero.
 */
const SLOT_DIAGNOSI = { intestazione: null, contenuto: null } as const;

/** Le linguette, con l'etichetta che si legge. */
const SCHEDE: readonly (readonly [Scheda, string])[] = [
  ["json", "skin.json"],
  ["css", "CSS compilato"],
  ["diff", "Diff con l'originale"],
];

/** Il colore di un avviso, per gravità. */
function tinta(kind: Avviso["kind"]): string {
  switch (kind) {
    case "contrast":
      return "var(--danger)";
    case "costBudget":
    case "unkeptCapability":
      return "var(--warning)";
    default:
      return "var(--color-text-3)";
  }
}

/** Una riga della tabella dei contrasti. */
function RigaContrasto({ c, soglia }: { c: Contrasto; soglia: number }) {
  const colore = c.passa ? "var(--success)" : "var(--danger)";
  return (
    <div className="riga-contrasto">
      <Icona nome={c.passa ? "i-check" : "i-alert"} dim={13} />
      <span className="coppia">
        {c.davanti.replace("color.", "")} su {c.dietro.replace("color.", "")}
      </span>
      <span className="rapporto" style={{ color: colore }}>
        {c.scuro.toFixed(2)}
      </span>
      <span
        className="rapporto"
        style={{
          color:
            c.chiaro === null
              ? "var(--color-text-3)"
              : c.chiaro >= soglia
                ? "var(--success)"
                : "var(--danger)",
        }}
      >
        {c.chiaro === null ? "—" : c.chiaro.toFixed(2)}
      </span>
    </div>
  );
}

export function Documento({
  sorgente,
  onSorgente,
  esito,
  ultimoValido,
  scheda,
  onScheda,
  soglia,
  originale,
  onCorreggi,
  registro,
  idSkin,
  colonnaSinistra,
  vaiAlla,
  onArrivato,
}: {
  sorgente: string;
  onSorgente: (testo: string) => void;
  esito: Validazione | null;
  /** L'ultimo foglio compilato con successo: l'anteprima resta lì. */
  ultimoValido: Validazione | null;
  scheda: Scheda;
  onScheda: (s: Scheda) => void;
  soglia: number;
  /** Il documento da cui si è partiti, per il confronto. */
  originale: string;
  /** Applica il suggerimento di `nearest_parts()`. */
  onCorreggi: (sbagliato: string, giusto: string) => void;
  /** Per la striscia dei conteggi: quant'è grande il vocabolario. */
  registro: Registro | null;
  idSkin: string;
  /** Il pacchetto e le istantanee. Lo compone lo Studio, che ha l'`id`. */
  colonnaSinistra: React.ReactNode;
  /** Un percorso da raggiungere col cursore, da un bottone «vai». */
  vaiAlla: string | null;
  onArrivato: () => void;
}) {
  const errore: Problema | null = esito?.errori[0] ?? null;
  const editor = useRef<HTMLTextAreaElement>(null);
  const sotto = useRef<HTMLPreElement>(null);
  const [dove, setDove] = useState({ riga: 1, colonna: 1 });

  const righe = useMemo(() => sorgente.split("\n"), [sorgente]);
  const rigaRotta = useMemo(
    () => (errore === null ? null : rigaDi(sorgente, errore.path)),
    [sorgente, errore],
  );

  /** Il cursore si legge dopo ogni cosa che lo può muovere. */
  const segnaPosizione = () => {
    const nodo = editor.current;
    if (nodo) setDove(posizione(sorgente, nodo.selectionStart));
  };

  /**
   * Il bottone «vai» porta il cursore, non solo la vista.
   *
   * Selezionare la chiave invece di posare il cursore accanto: quel che si è
   * venuti a cambiare è quello, e trovarlo già selezionato risparmia il doppio
   * clic che si farebbe comunque.
   */
  useEffect(() => {
    if (vaiAlla === null || scheda !== "json") return;
    const nodo = editor.current;
    if (!nodo) return;
    const pezzi = vaiAlla.split(".");
    for (let da = 0; da < pezzi.length; da += 1) {
      const cercato = `"${pezzi.slice(da).join(".")}"`;
      const dove = sorgente.indexOf(cercato);
      if (dove < 0) continue;
      nodo.focus();
      nodo.setSelectionRange(dove, dove + cercato.length);
      // Portare la riga in vista: `setSelectionRange` da solo non scorre se la
      // textarea aveva già il fuoco.
      const riga = sorgente.slice(0, dove).split("\n").length - 1;
      nodo.scrollTop = Math.max(0, riga * ALTEZZA_RIGA - nodo.clientHeight / 3);
      setDove(posizione(sorgente, dove));
      break;
    }
    onArrivato();
  }, [vaiAlla, scheda, sorgente, onArrivato]);

  /**
   * Il confronto con l'originale, riga per riga.
   *
   * Un confronto vero — con le mosse e i blocchi — sarebbe una dipendenza in
   * più per una scheda che serve a rispondere a una domanda sola: «cosa ho
   * cambiato». Riga per riga la risponde, e il documento è testo apposta perché
   * quando la domanda diventa più grande ci sia già `git diff`.
   */
  const differenze = useMemo(() => {
    const prima = originale.split("\n");
    const dopo = sorgente.split("\n");
    const quante = Math.max(prima.length, dopo.length);
    const righe: { segno: "=" | "+" | "-"; testo: string }[] = [];
    for (let i = 0; i < quante; i += 1) {
      const a = prima[i];
      const b = dopo[i];
      if (a === b) {
        if (a !== undefined) righe.push({ segno: "=", testo: a });
        continue;
      }
      if (a !== undefined) righe.push({ segno: "-", testo: a });
      if (b !== undefined) righe.push({ segno: "+", testo: b });
    }
    return righe;
  }, [originale, sorgente]);

  return (
    <div className="studio-documento">
      {colonnaSinistra}

      <div className="colonna-editor">
        {/* Linguette vere e non un segmentato: si saldano al riquadro sotto,
            e la forma dice che quel che si sceglie è il contenuto di **quel**
            riquadro e non di tutta la vista. */}
        <div className="linguette-editor" role="tablist" aria-label="Cosa mostrare">
          {SCHEDE.map(([chiave, etichetta]) => (
            <button
              key={chiave}
              type="button"
              role="tab"
              className="linguetta-file"
              aria-selected={scheda === chiave}
              data-active={scheda === chiave || undefined}
              onClick={() => onScheda(chiave)}
            >
              {etichetta}
            </button>
          ))}
          <span className="spinta" />
          <span className="misure">
            {scheda === "json"
              ? `riga ${dove.riga} · col ${dove.colonna}`
              : `${righe.length} righe · ${esito?.compilatoMs ?? 0} ms`}
          </span>
        </div>

        <div className="riquadro-editor">
          {scheda === "json" && (
            /*
             * Due strati sovrapposti: sotto un `<pre>` colorato, sopra la
             * `<textarea>` col testo trasparente e il cursore visibile. È il
             * modo di avere numeri di riga ed evidenziazione senza portarsi
             * dentro un editor intero — e la textarea resta una textarea, quindi
             * l'annulla, l'incolla e la selezione funzionano da soli.
             */
            <div className="editor-doppio">
              <pre className="sotto-editor" ref={sotto} aria-hidden="true">
                {righe.map((riga, i) => (
                  <div key={i} className="riga-json" data-rotta={i === rigaRotta || undefined}>
                    <span className="numero">{i + 1}</span>
                    <span className="testo">
                      {evidenzia(riga).map((pezzo, j) => (
                        <span key={j} className={`t-${pezzo.genere}`}>
                          {pezzo.testo}
                        </span>
                      ))}
                    </span>
                  </div>
                ))}
              </pre>
              <textarea
                ref={editor}
                className="editor field-input"
                value={sorgente}
                spellCheck={false}
                onChange={(e) => {
                  onSorgente(e.target.value);
                  segnaPosizione();
                }}
                onKeyUp={segnaPosizione}
                onClick={segnaPosizione}
                onScroll={(e) => {
                  const quale = sotto.current;
                  if (!quale) return;
                  quale.scrollTop = e.currentTarget.scrollTop;
                  quale.scrollLeft = e.currentTarget.scrollLeft;
                }}
                aria-label="Il documento della skin"
              />
            </div>
          )}
          {scheda === "css" && (
            <pre className="uscita">
              <code>
                {(esito?.errori.length ?? 0) > 0
                  ? (ultimoValido?.css ??
                    "/* Nessun foglio: il documento non è ancora valido. */")
                  : (esito?.css ?? "")}
              </code>
            </pre>
          )}
          {scheda === "diff" && (
            <pre className="uscita differenze">
              {differenze.map((r, i) => (
                <div key={i} className={`d-${r.segno === "=" ? "uguale" : r.segno === "+" ? "piu" : "meno"}`}>
                  <span className="segno">{r.segno === "=" ? " " : r.segno}</span>
                  <code>{r.testo}</code>
                </div>
              ))}
            </pre>
          )}

          {errore && (
            <div className="pannello-errore">
              <Icona nome="i-alert" dim={14} />
              <div className="dentro">
                <div className="dove">
                  <code className="codice">{errore.code}</code>
                  {errore.path.length > 0 && <> · <code>{errore.path}</code></>}
                </div>
                <div className="cosa">{errore.message}</div>
                {errore.forse.length > 0 && (
                  <div className="forse">
                    Forse{" "}
                    {errore.forse.map((nome) => (
                      <button
                        key={nome}
                        type="button"
                        className="suggerimento"
                        onClick={() => {
                          const sbagliato = errore.path.split(".").pop() ?? "";
                          onCorreggi(sbagliato, nome);
                        }}
                      >
                        {nome}
                      </button>
                    ))}
                    ?
                    <div className="da-dove">
                      Il suggerimento viene da <code>nearest_parts()</code>: se non
                      c&apos;è niente di simile non propone niente.
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Quant'è grande il vocabolario, accanto a quanto se ne è sbagliato:
              «47 token noti» è la risposta alla domanda che viene subito dopo
              «questo nome non esiste» — cioè quali esistono. */}
          <div className="piede-editor">
            <span style={{ color: (esito?.errori.length ?? 0) > 0 ? "var(--danger)" : undefined }}>
              {esito?.errori.length ?? 0} errori
            </span>
            <span style={{ color: (esito?.avvisi.length ?? 0) > 0 ? "var(--warning)" : undefined }}>
              {esito?.avvisi.length ?? 0} avvisi
            </span>
            <span>{registro?.tokens.length ?? 0} token noti</span>
            <span>{registro?.parts.length ?? 0} parti note</span>
            <span>{registro?.effects.length ?? 0} effetti</span>
            <span className="spinta" />
            <span className="deterministico">
              l&apos;uscita è deterministica: stessa skin, stesso CSS
            </span>
          </div>
        </div>
      </div>

      <aside className="colonna-diagnosi">
        {/*
         * L'anteprima sta anche qui, e qui serve più che altrove: è mentre si
         * scrive che il documento si rompe, ed è qui che «resta all'ultimo stato
         * valido» smette di essere una promessa e diventa una cosa che si vede.
         */}
        <section className="anteprima-documento">
          <h3>Anteprima · l&apos;ultimo stato valido</h3>
          <div className="riquadro-fermo" data-in-pausa={errore !== null || undefined}>
            <Anteprima
              css={errore !== null ? (ultimoValido?.css ?? "") : (esito?.css ?? "")}
              id={idSkin}
              parti={SENZA_REGISTRO}
              sondaAccesa={false}
              scelta={null}
              onScegli={() => undefined}
            >
              <Impaginazione
                albero={
                  errore !== null
                    ? (ultimoValido?.layout?.shell ?? null)
                    : (esito?.layout?.shell ?? null)
                }
                contesto={contestoFinto("libreria")}
                slot={SLOT_DIAGNOSI}
              />
            </Anteprima>
            {errore !== null && (
              <span className="velo">
                <span className="pillola-pausa">in pausa sull&apos;errore</span>
              </span>
            )}
          </div>
        </section>

        <section>
          <h3>check_skin() · {esito?.avvisi.length ?? 0} avvisi</h3>
          {(esito?.avvisi.length ?? 0) === 0 ? (
            <p className="niente">Nessun avviso.</p>
          ) : (
            <div className="avvisi">
              {esito?.avvisi.map((a, i) => (
                <div key={i} className="avviso">
                  <span className="barretta" style={{ background: tinta(a.kind) }} />
                  <div className="dentro">
                    <div className="testa-avviso">
                      <span className="genere" style={{ color: tinta(a.kind) }}>
                        {a.kind}
                      </span>
                      <code className="dove">{a.path}</code>
                    </div>
                    <div className="cosa">{a.message}</div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        <section>
          <h3>
            Contrasto <span className="temi">scuro / chiaro</span>
          </h3>
          <div className="contrasti">
            {(esito?.contrasti ?? ultimoValido?.contrasti ?? []).map((c, i) => (
              <RigaContrasto key={i} c={c} soglia={soglia} />
            ))}
          </div>
          <p className="nota">
            <code>contrast_ratio()</code> tiene conto dell&apos;opacità:{" "}
            <code>text.3</code> è bianco al 46% <i>sopra</i> la superficie —
            misurarlo pieno darebbe sempre 21:1.
          </p>
        </section>
      </aside>
    </div>
  );
}
