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
import { numero } from "../formato";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";

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
  if (collegamento.inAttesa === 0 && collegamento.abbandonati === 0)
    return null;
  return (
    <p className="nota">
      {collegamento.inAttesa > 0 && (
        <>{t("scrobble.queued", { n: collegamento.inAttesa })}. </>
      )}
      {collegamento.abbandonati > 0 && (
        <>
          <strong>
            {t("scrobble.stuck", { n: collegamento.abbandonati })}
          </strong>
          {t("scrobble.stuck.after")}
          <button type="button" className="collegamento" onClick={onRiprova}>
            {t("scrobble.stuck.requeue")}
          </button>
          {t("scrobble.stuck.or")}
          <button type="button" className="collegamento" onClick={onDimentica}>
            {t("scrobble.stuck.drop")}
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
        if (esito.mandati > 0)
          pezzi.push(t("scrobble.sent", { n: esito.mandati }));
        if (esito.ignorati > 0)
          pezzi.push(t("scrobble.ignored", { n: esito.ignorati }));
        if (esito.inAttesa > 0)
          pezzi.push(t("scrobble.stillQueued", { n: esito.inAttesa }));
        onNotizia(
          pezzi.length > 0
            ? `${pezzi.join(", ")}${esito.motivi.length > 0 ? ` — ${esito.motivi.join("; ")}` : ""}`
            : t("scrobble.nothingToSend"),
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
              ? t("scrobble.enqueued", { n: numero(accodati) })
              : t("scrobble.nothingToQueue"),
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
        <Trans
          k="scrobble.p1"
          v={{ regola: <strong>{t("scrobble.p1.rule")}</strong> }}
        />
      </p>
      <p className="nota">
        <Trans
          k="scrobble.p2"
          v={{ nonPerde: <strong>{t("scrobble.p2.notLost")}</strong> }}
        />
      </p>

      <div className="riga-opzione">
        <div className="che-cosa">
          <div className="etichetta">{t("scrobble.toggle")}</div>
          <div className="spiegazione">
            {collegato ? t("scrobble.toggle.on") : t("scrobble.toggle.off")}
          </div>
        </div>
        <button
          type="button"
          className="interruttore switch"
          role="switch"
          aria-checked={stato?.attivo ?? false}
          aria-label={t("scrobble.toggle")}
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
      <p className="nota">{t("scrobble.lb.note")}</p>

      {lb?.collegato ? (
        <>
          <dl className="numeri">
            <div>
              <dt>{t("scrobble.account")}</dt>
              <dd className="stat-number">
                {lb.utente ?? t("scrobble.connected")}
              </dd>
            </div>
            <div>
              <dt>{t("scrobble.inQueue")}</dt>
              <dd className="stat-number">{numero(lb.inAttesa)}</dd>
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
              {inVolo ? t("scrobble.sending") : t("scrobble.sendNow")}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => esegui(ipc.scrobbleListenbrainzScollega)}
            >
              {t("scrobble.disconnect")}
            </button>
          </div>
        </>
      ) : (
        <div className="azioni">
          <input
            type="password"
            className="campo field-input"
            placeholder={t("scrobble.token")}
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
            {inVolo ? t("scrobble.checking") : t("scrobble.connect")}
          </button>
        </div>
      )}

      {lb?.collegato && (
        <details className="non-ritrovati">
          <summary>{t("scrobble.backlog")}</summary>
          <p>{t("scrobble.backlog.p1")}</p>
          <p className="nota">{t("scrobble.backlog.p2")}</p>
          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => importaCronologia(true)}
            >
              {t("scrobble.backlog.only")}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => importaCronologia(false)}
            >
              {t("scrobble.backlog.all")}
            </button>
          </div>
        </details>
      )}

      {/* ── Last.fm ── */}
      <h3 className="titoletto">Last.fm</h3>
      <p className="nota">
        <Trans
          k="scrobble.lfm.note"
          v={{ sito: <code>last.fm/api/account/create</code> }}
        />
      </p>

      {lfm?.collegato ? (
        <>
          <dl className="numeri">
            <div>
              <dt>{t("scrobble.account")}</dt>
              <dd className="stat-number">
                {lfm.utente ?? t("scrobble.connected")}
              </dd>
            </div>
            <div>
              <dt>{t("scrobble.inQueue")}</dt>
              <dd className="stat-number">{numero(lfm.inAttesa)}</dd>
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
              {t("scrobble.disconnect")}
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="azioni">
            <input
              type="text"
              className="campo field-input"
              placeholder={t("scrobble.lfm.key")}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
            />
            <input
              type="password"
              className="campo field-input"
              placeholder={t("scrobble.lfm.secret")}
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
              {t("scrobble.lfm.use")}
            </button>
          </div>

          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={!(lfm?.configurato ?? false) || inVolo}
              title={lfm?.configurato ? undefined : t("scrobble.lfm.needCreds")}
              onClick={() => esegui(ipc.scrobbleLastfmCollega)}
            >
              <Icona nome="i-list" dim={15} />
              {t("scrobble.lfm.authorize")}
            </button>
            {(stato?.attesaLastfm ?? false) && (
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={inVolo}
                onClick={() => esegui(ipc.scrobbleLastfmCompleta)}
              >
                {t("scrobble.lfm.done")}
              </button>
            )}
          </div>

          {(stato?.attesaLastfm ?? false) && (
            <p className="nota">
              <Trans
                k="scrobble.lfm.waiting"
                v={{
                  browser: <strong>{t("scrobble.lfm.waiting.browser")}</strong>,
                }}
              />
            </p>
          )}
        </>
      )}
    </>
  );
}
