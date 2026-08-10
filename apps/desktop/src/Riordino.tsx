/**
 * Il riordino della libreria sul disco.
 *
 * È l'unica schermata di Aether che **muove i file dell'utente**, ed è
 * costruita attorno a quel fatto: si vede l'elenco intero degli spostamenti
 * prima che ne avvenga uno, si conferma con un tasto che dice cosa fa, e alla
 * fine resta il modo di tornare indietro.
 *
 * L'elenco è per esteso e non un conteggio. Millequattrocento file spostati
 * sono una decisione che si prende guardandone qualcuno, non fidandosi di un
 * numero — e i gruppi «da rivedere» sono esattamente la parte che un conteggio
 * nasconderebbe.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import {
  ipc,
  testoErrore,
  type Avanzamento,
  type EsitoRiordino,
  type PianoRiordino,
} from "./ipc";

/** Come si legge un motivo per cui un brano resta dov'è. */
const MOTIVI: Record<string, string> = {
  alreadyInPlace: "già al posto giusto",
  outsideRoot: "fuori dalla cartella scelta",
  needsReview: "il suo gruppo è da rivedere",
  destinationTaken: "la destinazione è già occupata",
};

/** Il percorso senza la radice, che è uguale per tutti e ruba larghezza. */
function breve(percorso: string, radice: string): string {
  const senza = percorso.startsWith(radice)
    ? percorso.slice(radice.length)
    : percorso;
  return senza.replace(/^[/\\]+/, "");
}

export function Riordino({
  radice,
  onChiudi,
  onFatto,
}: {
  radice: string;
  onChiudi: () => void;
  onFatto: () => void;
}) {
  const [piano, setPiano] = useState<PianoRiordino | null>(null);
  const [esito, setEsito] = useState<EsitoRiordino | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(true);
  const [avanzamento, setAvanzamento] = useState<Avanzamento | null>(null);

  useEffect(() => {
    const promessa = listen<Avanzamento>("riordino:avanzamento", (evento) =>
      setAvanzamento(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    let annullato = false;
    ipc
      .pianoRiordino(radice)
      .then((p) => {
        if (!annullato) setPiano(p);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(testoErrore(e));
      })
      .finally(() => {
        if (!annullato) setInCorso(false);
      });
    return () => {
      annullato = true;
    };
  }, [radice]);

  const esegui = async () => {
    setInCorso(true);
    setErrore(null);
    setAvanzamento({ fatti: 0, totale: piano?.spostamenti.length ?? 0 });
    try {
      setEsito(await ipc.eseguiRiordino(radice));
      onFatto();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
      setAvanzamento(null);
    }
  };

  const annulla = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      setEsito(await ipc.annullaRiordino());
      onFatto();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
      setAvanzamento(null);
    }
  };

  const percentuale =
    avanzamento && avanzamento.totale > 0
      ? Math.round((avanzamento.fatti / avanzamento.totale) * 100)
      : 0;

  return (
    <div className="velo scuro" onClick={inCorso ? undefined : onChiudi}>
      <div
        className="finestrella larga"
        role="dialog"
        aria-modal="true"
        aria-label="Riordina la libreria"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{esito ? "Riordino concluso" : "Riordina la libreria"}</h2>
        <div className="percorso">{radice}</div>

        {errore && <div className="errore">{errore}</div>}

        {avanzamento && (
          <>
            <div className="avanzamento">
              <div style={{ width: `${percentuale}%` }} />
            </div>
            <div className="conteggio">
              {avanzamento.fatti} / {avanzamento.totale}
            </div>
          </>
        )}

        {!piano && !esito && inCorso && <p>Calcolo del piano…</p>}

        {esito && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>File spostati</span>
                <span className="conteggio">{esito.spostati}</span>
              </div>
              <div className="voce-rapporto">
                <span>Cartelle vuote rimosse</span>
                <span className="conteggio">{esito.cartelleRimosse}</span>
              </div>
            </div>
            {esito.falliti.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>{esito.falliti.length} spostamenti non riusciti</summary>
                <ul>
                  {esito.falliti.slice(0, 200).map((f) => (
                    <li key={f.da}>
                      {breve(f.da, radice)} — {f.errore.code}
                    </li>
                  ))}
                </ul>
              </details>
            )}
            <p>
              I file sono in cartelle nuove, ma il database ha ancora i percorsi
              vecchi. <strong>Fai una scansione</strong>: gli spostamenti vengono
              riconosciuti come tali, quindi ascolti, preferiti, valutazioni e
              playlist restano attaccati ai loro brani.
            </p>
          </>
        )}

        {piano && !esito && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>Brani considerati</span>
                <span className="conteggio">{piano.letti}</span>
              </div>
              <div className="voce-rapporto">
                <span>Da spostare</span>
                <span className="conteggio">{piano.spostamenti.length}</span>
              </div>
              {piano.fermi.map((f) => (
                <div className="voce-rapporto" key={f.motivo}>
                  <span>Fermi — {MOTIVI[f.motivo] ?? f.motivo}</span>
                  <span className="conteggio">{f.quanti}</span>
                </div>
              ))}
            </div>

            {piano.daRivedere.length > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {piano.daRivedere.length} gruppi che Aether non sa dove mettere
                </summary>
                <p>
                  Album i cui tag non concordano abbastanza per decidere un
                  artista. Non vengono toccati: sistemare i tag e rifare il piano
                  è l&apos;unico modo di includerli, ed è meglio di indovinare.
                </p>
                <ul>
                  {piano.daRivedere.slice(0, 100).map((r) => (
                    <li key={r.album}>
                      «{r.album}» — {r.brani} brani, {r.artisti.length} artisti:{" "}
                      {r.artisti.slice(0, 3).join(", ")}
                      {r.artisti.length > 3 ? ", …" : ""}
                    </li>
                  ))}
                </ul>
              </details>
            )}

            {piano.spostamenti.length > 0 && (
              <details className="spostamenti" open>
                <summary>{piano.spostamenti.length} spostamenti</summary>
                <ul>
                  {piano.spostamenti.slice(0, 500).map((s) => (
                    <li key={s.da}>
                      <span className="da">{breve(s.da, radice)}</span>
                      <span className="freccia" aria-hidden="true">
                        ↦
                      </span>
                      <span className="a">{breve(s.a, radice)}</span>
                    </li>
                  ))}
                </ul>
                {piano.spostamenti.length > 500 && (
                  <p>
                    Mostrati i primi 500 di {piano.spostamenti.length}. Verranno
                    spostati tutti.
                  </p>
                )}
              </details>
            )}
          </>
        )}

        <div className="tasti-finestrella">
          {(piano?.annullabile || esito?.annullabile) && (
            <button
              type="button"
              className="bottone"
              disabled={inCorso}
              onClick={() => void annulla()}
            >
              Annulla l&apos;ultimo riordino
            </button>
          )}
          <button
            type="button"
            className="bottone"
            disabled={inCorso}
            onClick={onChiudi}
          >
            {esito ? "Chiudi" : "Non fare niente"}
          </button>
          {!esito && (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || !piano || piano.spostamenti.length === 0}
              onClick={() => void esegui()}
            >
              {inCorso
                ? "Sposto…"
                : `Sposta ${piano?.spostamenti.length ?? 0} file`}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
