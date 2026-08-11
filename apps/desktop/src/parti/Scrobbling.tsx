/**
 * Lo scrobbling: mandare a un servizio esterno quel che si ascolta.
 *
 * # Perché è un componente suo e non un blocco dentro Impostazioni
 *
 * Perché ha uno stato che nessun altro guarda — due collegamenti, due code, un
 * consenso a metà — e perché quello stato si rilegge dopo **ogni** gesto: tutti
 * i comandi restituiscono lo stato completo apposta, così non esiste un istante
 * in cui la schermata mostra la situazione precedente. Tenerlo in `App` vorrebbe
 * dire far viaggiare sei prop e un aggiornatore per una schermata che si apre
 * dieci volte l'anno.
 *
 * # I due servizi non si somigliano, e la schermata lo dice
 *
 * ListenBrainz chiede un token da incollare, e basta. Last.fm chiede di
 * registrare un'applicazione, di incollare chiave **e** segreto, e poi un
 * consenso nel browser che va confermato a mano al ritorno — perché Last.fm non
 * richiama nessuno: fra l'apertura del browser e il «ho autorizzato» non
 * succede niente di osservabile da qui.
 *
 * Nascondere questa differenza dietro due bottoni identici renderebbe il
 * secondo incomprensibile la volta in cui non funziona.
 */
import { useCallback, useEffect, useState } from "react";

import type { CollegamentoScrobble, EsitoInvio, StatoScrobble } from "../ipc";
import { ipc } from "../ipc";
import { Icona } from "./Icone";

/** Quanti ascolti aspettano, detto in italiano. */
function quanti(n: number, uno: string, molti: string): string {
  if (n === 1) return `1 ${uno}`;
  return `${n.toLocaleString("it")} ${molti}`;
}

/** La riga di stato di una coda: quanti aspettano, quanti sono fermi. */
function Coda({
  collegamento,
  onRiprova,
  onDimentica,
}: {
  collegamento: CollegamentoScrobble;
  onRiprova: () => void;
  onDimentica: () => void;
}) {
  if (collegamento.inAttesa === 0 && collegamento.abbandonati === 0) return null;
  return (
    <p className="nota">
      {collegamento.inAttesa > 0 && (
        <>{quanti(collegamento.inAttesa, "ascolto in coda", "ascolti in coda")}. </>
      )}
      {collegamento.abbandonati > 0 && (
        <>
          <strong>
            {quanti(
              collegamento.abbandonati,
              "ascolto si è fermato",
              "ascolti si sono fermati",
            )}
          </strong>{" "}
          dopo dieci tentativi.{" "}
          <button type="button" className="collegamento" onClick={onRiprova}>
            Rimettili in fila
          </button>{" "}
          oppure{" "}
          <button type="button" className="collegamento" onClick={onDimentica}>
            buttali
          </button>
          .
        </>
      )}
    </p>
  );
}

export function Scrobbling({
  onErrore,
  onNotizia,
}: {
  onErrore: (e: unknown) => void;
  /** Una cosa andata bene da dire: non è un errore e non va sul canale rosso. */
  onNotizia: (testo: string) => void;
}) {
  const [stato, setStato] = useState<StatoScrobble | null>(null);
  const [token, setToken] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [segreto, setSegreto] = useState("");
  const [inVolo, setInVolo] = useState(false);

  /**
   * Esegue un comando e ne prende lo stato di ritorno.
   *
   * Tutti i comandi dello scrobbling restituiscono lo stato completo, quindi
   * non serve mai una seconda chiamata per rileggerlo — che è anche l'unico
   * modo di non avere un istante in cui la schermata mostra quel che c'era
   * prima.
   */
  const esegui = useCallback(
    (azione: () => Promise<StatoScrobble>) => {
      setInVolo(true);
      azione()
        .then(setStato)
        .catch(onErrore)
        .finally(() => setInVolo(false));
    },
    [onErrore],
  );

  useEffect(() => {
    ipc.scrobbleStato().then(setStato).catch(onErrore);
  }, [onErrore]);

  const invia = useCallback(() => {
    setInVolo(true);
    ipc
      .scrobbleInvia()
      .then((esito: EsitoInvio) => {
        const pezzi: string[] = [];
        if (esito.mandati > 0) pezzi.push(`${esito.mandati} mandati`);
        if (esito.ignorati > 0) pezzi.push(`${esito.ignorati} scartati`);
        if (esito.inAttesa > 0) pezzi.push(`${esito.inAttesa} ancora in coda`);
        onNotizia(
          pezzi.length > 0
            ? `${pezzi.join(", ")}${esito.motivi.length > 0 ? ` — ${esito.motivi.join("; ")}` : ""}`
            : "Non c'era niente da mandare.",
        );
        return ipc.scrobbleStato();
      })
      .then(setStato)
      .catch(onErrore)
      .finally(() => setInVolo(false));
  }, [onErrore, onNotizia]);

  const importaCronologia = useCallback(
    (soloImportati: boolean) => {
      setInVolo(true);
      ipc
        .scrobbleImportaCronologia(soloImportati)
        .then((accodati: number) => {
          onNotizia(
            accodati > 0
              ? `${accodati.toLocaleString("it")} ascolti messi in coda verso ListenBrainz.`
              : "Non c'era niente da accodare: erano già tutti in coda o già mandati.",
          );
          return ipc.scrobbleStato();
        })
        .then(setStato)
        .catch(onErrore)
        .finally(() => setInVolo(false));
    },
    [onErrore, onNotizia],
  );

  const lb = stato?.listenbrainz;
  const lfm = stato?.lastfm;
  const collegato = (lb?.collegato ?? false) || (lfm?.collegato ?? false);

  return (
    <>
      <p className="nota">
        Manda quel che ascolti a un servizio che tiene la tua cronologia. Conta
        con la stessa regola di tutto il resto di Aether — <strong>metà brano
        o quattro minuti</strong>, quel che viene prima — quindi un brano saltato
        dopo tre secondi non esce, esattamente come non finisce nei tuoi «più
        ascoltati».
      </p>
      <p className="nota">
        Quel che non parte perché non c&apos;è rete <strong>non si perde</strong>:
        resta in una coda su disco e riparte da solo, anche dopo una chiusura.
      </p>

      <div className="riga-opzione">
        <div className="che-cosa">
          <div className="etichetta">Manda quel che ascolto</div>
          <div className="spiegazione">
            {collegato
              ? "Spento, la coda smette di riempirsi: non ti ritrovi tre giorni di ascolti spediti insieme alla riaccensione."
              : "Collega prima un servizio qui sotto"}
          </div>
        </div>
        <button
          type="button"
          className="interruttore switch"
          role="switch"
          aria-checked={stato?.attivo ?? false}
          aria-label="Manda quel che ascolto"
          disabled={!collegato || inVolo}
          onClick={() =>
            esegui(() => ipc.scrobbleAttivo(!(stato?.attivo ?? false)))
          }
        >
          <span className="pista switch-track" aria-hidden="true">
            <span className="pallina" />
          </span>
        </button>
      </div>

      {/* ── ListenBrainz ── */}
      <h3 className="titoletto">ListenBrainz</h3>
      <p className="nota">
        Di MetaBrainz, gli stessi di MusicBrainz. I dati sono pubblici e
        scaricabili: la tua cronologia resta tua anche se un giorno il servizio
        chiude. Serve solo un token, che sta nelle impostazioni del tuo account.
      </p>

      {lb?.collegato ? (
        <>
          <dl className="numeri">
            <div>
              <dt>Account</dt>
              <dd className="stat-number">{lb.utente ?? "collegato"}</dd>
            </div>
            <div>
              <dt>In coda</dt>
              <dd className="stat-number">{lb.inAttesa.toLocaleString("it")}</dd>
            </div>
          </dl>
          <Coda
            collegamento={lb}
            onRiprova={() => esegui(ipc.scrobbleRiprova)}
            onDimentica={() => esegui(ipc.scrobbleDimentica)}
          />
          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={invia}
            >
              <Icona nome="i-cloud" dim={15} />
              {inVolo ? "Invio in corso…" : "Manda adesso"}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => esegui(ipc.scrobbleListenbrainzScollega)}
            >
              Scollega
            </button>
          </div>
        </>
      ) : (
        <div className="azioni">
          <input
            type="password"
            className="campo"
            placeholder="token utente"
            value={token}
            onChange={(e) => setToken(e.target.value)}
          />
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={token.trim().length === 0 || inVolo}
            onClick={() =>
              esegui(() =>
                ipc.scrobbleListenbrainzCollega(token).then((s: StatoScrobble) => {
                  setToken("");
                  return s;
                }),
              )
            }
          >
            {inVolo ? "Verifico…" : "Collega"}
          </button>
        </div>
      )}

      {lb?.collegato && (
        <details className="non-ritrovati">
          <summary>Manda anche la cronologia che hai già</summary>
          <p>
            ListenBrainz è l&apos;unico dei due che accetta ascolti vecchi in
            blocco: se hai importato la cronologia da Spotify, sono anni di
            ascolti che possono diventare la tua cronologia su un servizio che
            non appartiene a nessuna piattaforma.
          </p>
          <p className="nota">
            Non manda niente due volte: quel che è già uscito non torna in coda.
            Su Last.fm questo non si può fare — rifiuta gli ascolti con una data
            vecchia e ha un tetto giornaliero.
          </p>
          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => importaCronologia(true)}
            >
              Solo quella importata da Spotify
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => importaCronologia(false)}
            >
              Tutta
            </button>
          </div>
        </details>
      )}

      {/* ── Last.fm ── */}
      <h3 className="titoletto">Last.fm</h3>
      <p className="nota">
        Serve un&apos;applicazione registrata a tuo nome — Last.fm non permette
        di distribuirne una dentro un programma. Si crea in un minuto da{" "}
        <code>last.fm/api/account/create</code>, e dà una chiave e un segreto da
        incollare qui.
      </p>

      {lfm?.collegato ? (
        <>
          <dl className="numeri">
            <div>
              <dt>Account</dt>
              <dd className="stat-number">{lfm.utente ?? "collegato"}</dd>
            </div>
            <div>
              <dt>In coda</dt>
              <dd className="stat-number">{lfm.inAttesa.toLocaleString("it")}</dd>
            </div>
          </dl>
          <Coda
            collegamento={lfm}
            onRiprova={() => esegui(ipc.scrobbleRiprova)}
            onDimentica={() => esegui(ipc.scrobbleDimentica)}
          />
          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => esegui(ipc.scrobbleLastfmScollega)}
            >
              Scollega
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="azioni">
            <input
              type="text"
              className="campo"
              placeholder="chiave (api key)"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
            />
            <input
              type="password"
              className="campo"
              placeholder="segreto"
              value={segreto}
              onChange={(e) => setSegreto(e.target.value)}
            />
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() =>
                esegui(() =>
                  ipc.scrobbleLastfmCredenziali(apiKey, segreto).then((s: StatoScrobble) => {
                    setSegreto("");
                    return s;
                  }),
                )
              }
            >
              Usa queste
            </button>
          </div>

          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={!(lfm?.configurato ?? false) || inVolo}
              title={
                lfm?.configurato
                  ? undefined
                  : "Incolla prima chiave e segreto qui sopra"
              }
              onClick={() => esegui(ipc.scrobbleLastfmCollega)}
            >
              <Icona nome="i-list" dim={15} />
              Autorizza nel browser
            </button>
            {(stato?.attesaLastfm ?? false) && (
              <button
                type="button"
                className="bottone btn-primary"
                disabled={inVolo}
                onClick={() => esegui(ipc.scrobbleLastfmCompleta)}
              >
                Ho autorizzato, completa
              </button>
            )}
          </div>

          {(stato?.attesaLastfm ?? false) && (
            <p className="nota">
              Ho aperto la pagina di Last.fm nel <strong>browser di
              sistema</strong>. Dai il consenso lì, poi torna qui e premi «Ho
              autorizzato»: Last.fm non avvisa nessuno quando hai finito, quindi
              questo passo va fatto a mano. Il permesso vale un&apos;ora.
            </p>
          )}
        </>
      )}
    </>
  );
}
