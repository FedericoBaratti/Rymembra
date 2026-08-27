/**
 * Gli aggiornamenti: la fascia che avvisa e la scheda che comanda.
 *
 * # Perché due componenti in un file solo
 *
 * Perché guardano lo stesso stato e devono dire la stessa cosa. La fascia
 * compare sopra qualunque schermata quando c'è una versione nuova; la scheda
 * sta in Impostazioni e serve a spegnere il controllo, a farne partire uno a
 * mano, e a vedere quale versione è installata — che, fino a oggi, l'interfaccia
 * non diceva da nessuna parte. Tenerle in due file vorrebbe dire due copie del
 * `listen` e due idee di quando l'avviso vada nascosto.
 *
 * # Perché lo stato non passa da `App`
 *
 * Stessa ragione di `Scrobbling`: è uno stato che nessun altro guarda, e ogni
 * comando restituisce quello nuovo, quindi non esiste un istante in cui la
 * schermata mostra la situazione di prima. Farlo scendere da `App` vorrebbe
 * dire tre prop e un aggiornatore per una cosa che si tocca due volte l'anno.
 *
 * # La fascia non torna dopo un «non ora»
 *
 * Il rifiuto si scrive nel database, non in un `useState`: un avviso che
 * riappare a ogni riavvio della finestra — o ogni mezz'ora, che è la stessa
 * cosa detta più spesso — è un avviso che si impara a chiudere senza leggerlo.
 */
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import type { AvanzamentoAggiornamento, StatoAggiornamenti } from "../ipc";
import { ipc, testoErrore } from "../ipc";
import { dataOra } from "../formato";
import { t } from "../lingue";
import { Icona } from "./Icone";
import { Interruttore } from "./Interruttore";

/**
 * Lo stato del controllo aggiornamenti, tenuto vivo dagli eventi.
 *
 * Una lettura all'apertura e poi solo `aggiornamenti:stato`: il controllo lo fa
 * un filo di sottofondo ogni mezz'ora, e la finestra non ha modo — né motivo —
 * di sapere quando.
 */
function useAggiornamenti(
  onErrore: (e: unknown) => void,
): [StatoAggiornamenti | null, (s: StatoAggiornamenti) => void] {
  const [stato, setStato] = useState<StatoAggiornamenti | null>(null);

  useEffect(() => {
    ipc.aggiornamentiStato().then(setStato).catch(onErrore);
  }, [onErrore]);

  useEffect(() => {
    const promessa = listen<StatoAggiornamenti>(
      "aggiornamenti:stato",
      (evento) => setStato(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  return [stato, setStato];
}

/** Da byte a una misura che si legge. Nessun decimale sotto il megabyte. */
function peso(byte: number): string {
  const mb = byte / (1024 * 1024);
  if (mb < 1) return t("update.size.kb", { n: Math.round(byte / 1024) });
  return t("update.size.mb", { n: mb.toFixed(1) });
}

/**
 * La fascia che avvisa che c'è una versione nuova.
 *
 * Non compare mai da sola in mezzo a niente: sta dove stanno l'errore e la
 * notizia, in cima al contenuto, e si comporta come loro.
 */
export function AvvisoAggiornamento({
  onErrore,
}: {
  onErrore: (e: unknown) => void;
}) {
  const [stato, setStato] = useAggiornamenti(onErrore);
  const [avanzamento, setAvanzamento] =
    useState<AvanzamentoAggiornamento | null>(null);

  useEffect(() => {
    const promessa = listen<AvanzamentoAggiornamento>(
      "aggiornamenti:avanzamento",
      (evento) => setAvanzamento(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  const disponibile = stato?.disponibile ?? null;

  /**
   * Qualcuno ha premuto «Aggiorna» in questa sessione della fascia.
   *
   * Serve a distinguere due guasti che arrivano dallo stesso campo. Un
   * controllo periodico che non trova la rete finisce anche lui in
   * `stato.errore`, e mostrarlo qui vorrebbe dire una fascia rossa ogni volta
   * che il wifi cade — per una cosa che si ritenta da sola fra mezz'ora e che
   * nessuno deve fare. Uno scaricamento fallito è l'opposto: è la risposta a un
   * gesto, e chi lo ha fatto sta guardando.
   */
  const [tentato, setTentato] = useState(false);

  const installa = useCallback(() => {
    setTentato(true);
    ipc.aggiornamentiInstalla().catch(onErrore);
  }, [onErrore]);

  const salta = useCallback(() => {
    if (disponibile === null) return;
    ipc
      .aggiornamentiSalta(disponibile.versione)
      .then(setStato)
      .catch(onErrore);
  }, [disponibile, onErrore, setStato]);

  if (disponibile === null || disponibile.saltata) return null;

  const scaricando = stato?.installazione === true;
  // `totale` manca quando il server non manda `Content-Length`. Una barra che
  // non sa dove arriva è una barra che mente: in quel caso si mostrano i
  // megabyte scesi e basta.
  const totale = avanzamento?.totale ?? null;
  const quota =
    avanzamento !== null && totale !== null && totale > 0
      ? Math.min(100, (avanzamento.scaricati / totale) * 100)
      : null;

  return (
    <div className="notizia aggiornamento toast-card" role="status">
      <Icona nome="i-cloud" dim={16} />
      <span>
        <strong>{t("update.available", { v: disponibile.versione })}</strong>
        {disponibile.note !== null && (
          <details className="note-versione">
            <summary>{t("update.notes")}</summary>
            <p>{disponibile.note}</p>
          </details>
        )}
        {scaricando && (
          <span className="scaricamento">
            <span className="toast-progress">
              <span
                style={quota === null ? undefined : { width: `${quota}%` }}
              />
            </span>
            <span className="quanto">
              {avanzamento === null
                ? t("update.starting")
                : totale === null
                  ? peso(avanzamento.scaricati)
                  : `${peso(avanzamento.scaricati)} / ${peso(totale)}`}
            </span>
          </span>
        )}
        {tentato && !scaricando && stato?.errore != null && (
          <span className="guasto">{testoErrore(stato.errore)}</span>
        )}
      </span>
      {!scaricando && (
        <>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={installa}
          >
            {t("update.install")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("update.later")}
            title={t("update.later")}
            onClick={salta}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </>
      )}
    </div>
  );
}

/** La scheda in Impostazioni: l'interruttore, la versione, l'ultimo controllo. */
export function Aggiornamenti({
  onErrore,
}: {
  onErrore: (e: unknown) => void;
}) {
  const [stato, setStato] = useAggiornamenti(onErrore);

  const accendi = useCallback(
    (attivo: boolean) => {
      ipc.aggiornamentiAttivo(attivo).then(setStato).catch(onErrore);
    },
    [onErrore, setStato],
  );

  const adesso = useCallback(() => {
    ipc.aggiornamentiAdesso().then(setStato).catch(onErrore);
  }, [onErrore, setStato]);

  // Saltare «la versione vuota» è il modo di non saltarne nessuna: nessun
  // numero di versione è la stringa vuota, quindi il confronto di là non torna
  // mai e l'avviso ricompare. Un sesto comando per disfare quel che fa il
  // quinto sarebbe una riga di IPC in più per la stessa scrittura.
  const riprova = useCallback(() => {
    ipc.aggiornamentiSalta("").then(setStato).catch(onErrore);
  }, [onErrore, setStato]);

  return (
    <>
      <p className="nota">{t("settings.update.p1")}</p>
      <p className="nota">{t("settings.update.p2")}</p>

      {stato?.errore != null && (
        <div className="errore">{testoErrore(stato.errore)}</div>
      )}

      <Interruttore
        etichetta={t("settings.update.toggle")}
        spiegazione={t("settings.update.toggle.hint")}
        acceso={stato?.attivo ?? true}
        onCambia={accendi}
        impedito={
          stato?.configurato === false
            ? t("settings.update.unsigned")
            : undefined
        }
      />

      <dl className="numeri">
        <div>
          <dt>{t("settings.update.current")}</dt>
          <dd className="stat-number">{stato?.versioneCorrente ?? "—"}</dd>
        </div>
        <div>
          <dt>{t("settings.update.last")}</dt>
          <dd className="stat-number">
            {stato?.ultimoMs != null
              ? dataOra(stato.ultimoMs)
              : t("settings.when.never")}
          </dd>
        </div>
      </dl>

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={stato === null || stato.inCorso || !stato.configurato}
          onClick={adesso}
        >
          {stato?.inCorso === true
            ? t("settings.update.checking")
            : t("settings.update.check")}
        </button>
        {stato?.disponibile != null && stato.disponibile.saltata && (
          <button type="button" className="bottone btn-ghost" onClick={riprova}>
            {t("settings.update.unskip", { v: stato.disponibile.versione })}
          </button>
        )}
      </div>
    </>
  );
}
