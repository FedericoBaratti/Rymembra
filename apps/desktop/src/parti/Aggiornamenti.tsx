/**
 * Gli aggiornamenti: la fascia che avvisa e la scheda che comanda.
 *
 * # Perché due componenti in un file solo
 *
 * Perché guardano lo stesso stato e devono dire la stessa cosa. La fascia
 * compare sopra qualunque schermata quando c'è una versione nuova; la scheda
 * sta in Impostazioni e serve a spegnere il controllo, a farne partire uno a
 * mano, e a vedere quale versione è installata — che, fino a oggi, l'interfaccia
 * non diceva da nessuna parte. Tenerle in due file vorrebbe dire due copie
 * dell'ascolto e due idee di quando l'avviso vada nascosto.
 *
 * # Perché lo stato non passa da `App`
 *
 * Stessa ragione di `Scrobbling`: è uno stato che nessun altro guarda, e ogni
 * comando restituisce quello nuovo, quindi non esiste un istante in cui la
 * schermata mostra la situazione di prima. Farlo scendere da `App` vorrebbe
 * dire tre prop e un aggiornatore per una cosa che si tocca due volte l'anno.
 *
 * # «Non ora» e «Salta questa versione» sono due gesti
 *
 * Erano uno solo: la X della fascia si chiamava «Non ora» e scriveva nel
 * database che **quella versione** non andava più proposta. Chi voleva soltanto
 * finire di ascoltare un disco non la rivedeva mai più — per quella versione,
 * cioè fino alla prossima release — e senza sapere di aver deciso qualcosa.
 *
 * Adesso «Non ora» chiude la fascia per questa sessione, in un `useState`: al
 * prossimo avvio torna. «Salta questa versione» è il rifiuto vero, e si scrive
 * nel database, perché un avviso che riappare a ogni riavvio dopo che si è detto
 * di no è un avviso che si impara a chiudere senza leggerlo.
 */
import { useCallback, useEffect, useReducer, useState } from "react";

import type { AvanzamentoAggiornamento, StatoAggiornamenti } from "../ipc";
import { conCopiaLocale, type ConCopiaLocale } from "../documentoLocale";
import { ipc, testoErrore } from "../ipc";
import { dataOra, numero } from "../formato";
import { t } from "../lingue";
import { useAscolto } from "../pagine";
import { Icona } from "./Icone";
import { Annuncio } from "./Annuncio";
import { DocumentoLocale } from "./DocumentoLocale";
import { Interruttore } from "./Interruttore";
import { avanzamentoDopo } from "./avanzamentoAggiornamento";

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

  useAscolto<StatoAggiornamenti>("aggiornamenti:stato", setStato);

  return [stato, setStato];
}

/**
 * «Non ora», per il resto della sessione.
 *
 * Fuori dal componente e non solo nel suo `useState`: la fascia si smonta ogni
 * volta che la finestra cambia faccia — lo Studio delle skin prende il posto di
 * tutto — e rimontandosi ripartiva da capo, cioè l'avviso chiuso tornava. La
 * sessione è quella del processo, non quella del componente.
 */
let nonOraInSessione: string | null = null;

/** Da byte a una misura che si legge. Nessun decimale sotto il megabyte. */
function peso(byte: number): string {
  const mb = byte / (1024 * 1024);
  if (mb < 1) return t("update.size.kb", { n: Math.round(byte / 1024) });
  // `numero` e non `toFixed`: «12,5 MB» in italiano, non «12.5».
  return t("update.size.mb", { n: numero(mb, 1) });
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
  // Un tentativo nuovo, o uno annullato, riparte da zero: vedi
  // `avanzamentoDopo`.
  const [avanzamento, segnaAvanzamento] = useReducer(avanzamentoDopo, null);

  useAscolto<AvanzamentoAggiornamento>("aggiornamenti:avanzamento", (valore) =>
    segnaAvanzamento({ tipo: "avanzamento", valore }),
  );

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
    segnaAvanzamento({ tipo: "installa" });
    ipc.aggiornamentiInstalla().catch(onErrore);
  }, [onErrore]);

  /**
   * Ferma lo scaricamento. Non è un errore: la fascia torna com'era, con
   * «Aggiorna» di nuovo premibile, e lo stato arriva da `aggiornamenti:stato`.
   */
  const annulla = useCallback(() => {
    setTentato(false);
    segnaAvanzamento({ tipo: "annulla" });
    ipc.aggiornamentiAnnulla().catch(onErrore);
  }, [onErrore]);

  const salta = useCallback(() => {
    if (disponibile === null) return;
    ipc
      .aggiornamentiSalta(disponibile.versione)
      .then(setStato)
      .catch(onErrore);
  }, [disponibile, onErrore, setStato]);

  /** La versione chiusa con «Non ora» in questa sessione. Vedi il `/**` in testa. */
  const [nonOra, setNonOraQui] = useState<string | null>(nonOraInSessione);
  const setNonOra = (versione: string) => {
    nonOraInSessione = versione;
    setNonOraQui(versione);
  };

  const nascosta =
    disponibile === null ||
    disponibile.saltata ||
    disponibile.versione === nonOra;
  const scaricando = stato?.installazione === true;
  /**
   * Sul telefono, l'APK di questa versione non si installa sopra questa copia:
   * è firmato con un'altra chiave. «Aggiorna» non c'è — scaricherebbe decine di
   * megabyte che Android rifiuta — e al suo posto si dice come passare alla
   * versione pubblica senza perdere la libreria. Vedi `altraChiave`.
   */
  const altraChiave = disponibile?.altraChiave === true;
  const guasto = tentato && !scaricando && stato?.errore != null ? stato.errore : null;
  /*
   * Quel che si dice a chi ascolta lo schermo, da una regione sempre montata
   * (`Annuncio.tsx`): la versione nuova, e lo scaricamento che non è riuscito.
   * La fascia era lei stessa `role="status"` e nasceva già piena, cioè nel
   * solo modo in cui una regione viva spesso non dice niente. L'avanzamento non
   * si annuncia: cambia a ogni pacchetto, e lo dice la barra a chi la chiede.
   */
  const annuncio = (
    <Annuncio
      testo={
        nascosta
          ? null
          : guasto !== null
            ? testoErrore(guasto)
            : t("update.available", { v: disponibile.versione })
      }
    />
  );
  if (nascosta) return <>{annuncio}</>;

  // `totale` manca quando il server non manda `Content-Length`. Una barra che
  // non sa dove arriva è una barra che mente: in quel caso si mostrano i
  // megabyte scesi e basta.
  const totale = avanzamento?.totale ?? null;
  const quota =
    avanzamento !== null && totale !== null && totale > 0
      ? Math.min(100, (avanzamento.scaricati / totale) * 100)
      : null;

  return (
    <>
    {annuncio}
    <div className="notizia aggiornamento toast-card">
      <Icona nome="i-cloud" dim={16} />
      <span>
        <strong>{t("update.available", { v: disponibile.versione })}</strong>
        {disponibile.note !== null && (
          <details className="note-versione">
            <summary>{t("update.notes")}</summary>
            <p>{disponibile.note}</p>
          </details>
        )}
        {altraChiave && (
          <details className="note-versione">
            <summary>{t("update.otherKey", { v: disponibile.versione })}</summary>
            <p>{t("update.otherKey.steps", { v: disponibile.versione })}</p>
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
        {guasto !== null && <span className="guasto">{testoErrore(guasto)}</span>}
      </span>
      {scaricando && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          title={t("update.cancel.hint")}
          onClick={annulla}
        >
          {t("update.cancel")}
        </button>
      )}
      {!scaricando && (
        <>
          {!altraChiave && (
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={installa}
            >
              {t("update.install")}
            </button>
          )}
          <button
            type="button"
            className="bottone minuto btn-ghost"
            title={t("update.skip.hint")}
            onClick={salta}
          >
            {t("update.skip")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("update.later")}
            title={t("update.later.hint")}
            onClick={() => setNonOra(disponibile.versione)}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </>
      )}
    </div>
    </>
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

  /**
   * A che punto è il «Controlla adesso» premuto qui.
   *
   * Serve a dire «hai l'ultima versione», che prima non si diceva: il tasto
   * tornava cliccabile e basta, e chi l'aveva premuto non sapeva se il
   * controllo fosse andato o no. Tre passi e non un sì o un no, perché lo
   * stato che il comando restituisce arriva **prima** che il filo cominci — un
   * «non c'è niente» letto lì sarebbe la risposta di mezz'ora fa. Si dice solo
   * dopo aver visto il controllo cominciare e finire.
   */
  const [controllo, setControllo] = useState<"niente" | "chiesto" | "in-corso" | "finito">(
    "niente",
  );
  useEffect(() => {
    if (stato === null) return;
    setControllo((prima) => {
      if (prima === "chiesto" && stato.inCorso) return "in-corso";
      if (prima === "in-corso" && !stato.inCorso) return "finito";
      return prima;
    });
  }, [stato]);

  // La risposta del comando non si mette nello stato, e non per dimenticanza:
  // è scritta prima che il filo cominci, e l'evento con `inCorso` acceso può
  // arrivare **prima** di lei. Posata sopra, riportava `inCorso` a spento —
  // e il passo qui sopra leggeva «cominciato e finito» un controllo appena
  // partito, con un «hai l'ultima versione» detto sulla risposta di mezz'ora
  // fa. Quel che serve lo portano gli eventi, che sono in ordine.
  const adesso = useCallback(() => {
    setControllo("chiesto");
    ipc.aggiornamentiAdesso().catch(onErrore);
  }, [onErrore]);

  const aggiornato =
    controllo === "finito" &&
    stato !== null &&
    stato.disponibile === null &&
    stato.errore == null;

  // Saltare «la versione vuota» è il modo di non saltarne nessuna: nessun
  // numero di versione è la stringa vuota, quindi il confronto di là non torna
  // mai e l'avviso ricompare. Un sesto comando per disfare quel che fa il
  // quinto sarebbe una riga di IPC in più per la stessa scrittura.
  const riprova = useCallback(() => {
    ipc.aggiornamentiSalta("").then(setStato).catch(onErrore);
  }, [onErrore, setStato]);

  // Il diario sta qui e non in una scheda sua per una ragione sola: è la
  // sezione in cui si legge quale versione è installata, cioè il posto in cui
  // arriva chi sta per raccontare che qualcosa non funziona. Le due
  // informazioni che servono a una segnalazione stanno così a un centimetro
  // l'una dall'altra.
  const apriDiario = useCallback(() => {
    ipc.diarioApri().catch(onErrore);
  }, [onErrore]);

  /*
   * I quattro testi legali si leggono dalla copia che viaggia con l'app, nella
   * finestra e senza rete (D-10d); se questa installazione non la porta, il
   * sito come prima. Gli altri documenti sono pagine, e vanno al browser.
   */
  const [letto, setLetto] = useState<{ quale: ConCopiaLocale; titolo: string; testo: string } | null>(
    null,
  );
  const apriDocumento = useCallback(
    (quale: Parameters<typeof ipc.apriDocumento>[0], titolo: string) => {
      if (!conCopiaLocale(quale)) {
        ipc.apriDocumento(quale).catch(onErrore);
        return;
      }
      ipc
        .documentoLocale(quale)
        .then((testo) =>
          testo === null ? ipc.apriDocumento(quale) : setLetto({ quale, titolo, testo }),
        )
        .catch(onErrore);
    },
    [onErrore],
  );

  return (
    <>
      {stato?.errore != null && (
        <div className="errore">{testoErrore(stato.errore)}</div>
      )}

      <Interruttore
        etichetta={t("settings.update.toggle")}
        acceso={stato?.attivo ?? true}
        onCambia={accendi}
        impedito={
          stato?.gestitoDalStore === true
            ? t("settings.update.store")
            : stato?.configurato === false
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
          {/* Una data fra i numeri: la misura piccola di `numeri dd[data-quando]`
              (`impostazioni.css`), come l'ultima passata della sincronia. Grande
              andava a capo dopo la virgola, con l'ora da sola sotto. */}
          <dd className="stat-number" data-quando="">
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
        <button
          type="button"
          className="bottone btn-ghost"
          onClick={apriDiario}
        >
          {/* Sul telefono si condivide: vedi `settings.diary.open.telefono`. */}
          {t("settings.diary.open")}
        </button>
      </div>

      {aggiornato && (
        <p className="nota" role="status">
          {t("settings.update.upToDate", { v: stato.versioneCorrente })}
        </p>
      )}

      {/* I documenti, qui e non in una scheda loro: è la sezione in cui si
          legge quale versione è installata, cioè dove arriva chi sta per
          raccontare che qualcosa non va — e le due cose che gli servono, il
          numero di versione e il posto dove scrivere, stanno così accanto. */}
      <h3>{t("settings.docs.title")}</h3>
      <div className="azioni">
        {(
          [
            ["repository", "settings.docs.repository"],
            ["segnalazioni", "settings.docs.issues"],
            ["licenza", "settings.docs.license"],
            ["terze", "settings.docs.thirdParty"],
            ["privacy", "settings.docs.privacy"],
            ["condizioni", "settings.docs.terms"],
            ["caratteri", "settings.docs.fonts"],
          ] as const
        ).map(([quale, chiave]) => (
          <button
            key={quale}
            type="button"
            className="bottone btn-ghost"
            onClick={() => apriDocumento(quale, t(chiave))}
          >
            {t(chiave)}
          </button>
        ))}
      </div>
      {letto !== null && (
        <DocumentoLocale
          titolo={letto.titolo}
          testo={letto.testo}
          onSito={() => {
            ipc.apriDocumento(letto.quale).catch(onErrore);
          }}
          onChiudi={() => setLetto(null)}
        />
      )}
    </>
  );
}
