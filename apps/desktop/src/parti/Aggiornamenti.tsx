/**
 * Gli aggiornamenti: la scheda che avvisa e la scheda che comanda.
 *
 * # Perché due componenti in un file solo
 *
 * Perché guardano lo stesso stato e devono dire la stessa cosa. La scheda che
 * avvisa compare nella pila delle notifiche quando c'è una versione nuova e
 * racconta come procede l'aggiornamento; quella di Impostazioni serve a
 * spegnere il controllo, a farne partire uno a mano, e a vedere quale versione
 * è installata — che, fino a oggi, l'interfaccia non diceva da nessuna parte.
 * Tenerle in due file vorrebbe dire due copie dell'ascolto e due idee di
 * quando l'avviso vada nascosto.
 *
 * # Perché lo stato non passa da `App`
 *
 * Stessa ragione di `Scrobbling`: è uno stato che nessun altro guarda, e ogni
 * comando restituisce quello nuovo, quindi non esiste un istante in cui la
 * schermata mostra la situazione di prima. Farlo scendere da `App` vorrebbe
 * dire tre prop e un aggiornatore per una cosa che si tocca due volte l'anno.
 */
import { useCallback, useEffect, useReducer, useState } from "react";

import type {
  AvanzamentoAggiornamento,
  FaseAggiornamento,
  QuandoInstallare,
  StatoAggiornamenti,
} from "../ipc";
import { conCopiaLocale, type ConCopiaLocale } from "../documentoLocale";
import { ipc, testoErrore } from "../ipc";
import { dataOra, numero } from "../formato";
import { t } from "../lingue";
import { useAscolto } from "../pagine";
import { MOBILE } from "../piattaforma";
import { Presenza } from "../presenza";
import { Icona, type NomeIcona } from "./Icone";
import { Annuncio } from "./Annuncio";
import { DocumentoLocale } from "./DocumentoLocale";
import { Interruttore } from "./Interruttore";
import {
  attesaLunga,
  avanzamentoDopo,
  guastoDaMostrare,
  passiDi,
  quantoManca,
  SOLLECITO_MS,
  type ChiavePasso,
  type Forma,
} from "./avanzamentoAggiornamento";

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
 * Fuori dal componente e non solo nel suo `useState`: la scheda si smonta ogni
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

/** Dove gira la scheda: i passi sono quattro sul computer e tre sul telefono. */
const FORMA: Forma = MOBILE ? "telefono" : "computer";

/**
 * L'icona di ogni fase: nessun simbolo nuovo nello sprite, e `currentColor`
 * ovunque, cioè i colori della skin.
 */
const ICONA_DELLA_FASE: Record<FaseAggiornamento | "disponibile", NomeIcona> = {
  disponibile: "i-cloud",
  scarico: "i-import",
  verifico: "i-eye",
  installo: "i-sync",
  riavvio: "i-sync",
  pronto: "i-check",
};

/** Il nome breve di un passo, quello che sta nella fila. */
function nomeDelPasso(chiave: ChiavePasso): string {
  switch (chiave) {
    case "scarico":
      return t("update.step.download");
    case "verifico":
      return t("update.step.verify");
    case "installo":
      return t("update.step.install");
    case "riavvio":
      return t("update.step.restart");
    case "installa":
      return t("update.step.confirm");
  }
}

/** La frase che dice cosa sta succedendo, sotto la barra. */
function fraseDellaFase(fase: FaseAggiornamento, prontaDaTempo: boolean): string {
  switch (fase) {
    case "scarico":
      return t("update.status.download");
    case "verifico":
      return t("update.status.verify");
    case "installo":
      return t("update.status.install");
    case "riavvio":
      return t("update.status.restart");
    case "pronto":
      if (MOBILE) return t("update.status.ready.phone");
      return prontaDaTempo
        ? t("update.status.ready.late")
        : t("update.status.ready.close");
  }
}

/**
 * La scheda che avvisa che c'è una versione nuova, e racconta come procede.
 *
 * Sta nella pila delle notifiche (`PilaToast`), come `toast-card`, e non nel
 * flusso della pagina: nel flusso spingeva giù il titolo, l'indice e il filtro
 * proprio mentre si stava per cliccarci (R18-17), e un aggiornamento che dura
 * minuti merita un posto che non si muove.
 *
 * # I passi
 *
 * Sul computer: **Scarico** (percentuale, megabyte, velocità, tempo rimasto) →
 * **Verifico** la firma → **Installo** → **Riavvio**. Sul telefono: Scarico →
 * Verifico → **Installa**, con la schermata di conferma di Android. Il nucleo
 * dice a che punto è con `fase` (`StatoAggiornamenti`), e la scheda si
 * ricostruisce da lì a ogni apertura: con il motore nel vassoio la finestra può
 * chiudersi e riaprirsi nel mezzo di uno scaricamento, e gli eventi mandati
 * senza finestra si perdono. L'avanzamento dei byte, invece, è un evento a sé
 * (`aggiornamenti:avanzamento`), e il riduttore che ne ricava velocità e tempo
 * rimasto sta in `avanzamentoAggiornamento.ts`.
 *
 * # «Aggiorna» e «Alla chiusura» (computer)
 *
 * «Aggiorna» scarica, verifica e installa: l'installer chiude Rymembra e la
 * riapre. «Alla chiusura» scarica e verifica **adesso** — un gesto di chi
 * guarda: la regola «niente senza un tasto» regge — e installa quando si esce
 * davvero, senza riaprire. Con l'avvio con Windows e la X nel vassoio il
 * programma può non uscire mai: per questo dopo tre giorni la scheda ripropone
 * «Installa ora», e «Esci» dal vassoio installa.
 *
 * # «Non ora» e «Salta questa versione» sono due gesti
 *
 * Erano uno solo: la X della fascia si chiamava «Non ora» e scriveva nel
 * database che **quella versione** non andava più proposta. Chi voleva soltanto
 * finire di ascoltare un disco non la rivedeva mai più — per quella versione,
 * cioè fino alla prossima release — e senza sapere di aver deciso qualcosa.
 *
 * Adesso «Non ora» chiude la scheda per questa sessione: al prossimo avvio
 * torna. «Salta questa versione» è il rifiuto vero, e si scrive nel database,
 * perché un avviso che riappare a ogni riavvio dopo che si è detto di no è un
 * avviso che si impara a chiudere senza leggerlo.
 *
 * # Il movimento
 *
 * L'icona respira mentre il lavoro è in corso, e la barra scorre senza gradini
 * (`stile/libreria.css`). Tutto si spegne con `prefers-reduced-motion` e con
 * «Riduci tutto» (`movimento.css`), e nei colori forzati i passi restano
 * leggibili (`contrasto.css`).
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
    segnaAvanzamento({ tipo: "avanzamento", valore, adesso: Date.now() }),
  );

  const disponibile = stato?.disponibile ?? null;
  const fase: FaseAggiornamento | null =
    stato?.fase ?? (stato?.installazione === true ? "scarico" : null);
  const prontoDalMs = stato?.prontoDalMs ?? null;

  // Finito, annullato o fallito: i byte del tentativo di prima non restano.
  useEffect(() => {
    if (fase === null) segnaAvanzamento({ tipo: "annulla" });
  }, [fase]);

  /**
   * L'ora, per la sola ragione di sapere se il pacchetto aspetta da troppo.
   *
   * Si rinfresca una volta, quando il sollecito scade: tre giorni stanno dentro
   * il massimo di un `setTimeout`, e la scheda deve cambiare faccia senza che
   * nessuno la tocchi.
   */
  const [adesso, setAdesso] = useState(() => Date.now());
  useEffect(() => {
    if (prontoDalMs === null) return;
    const mancano = SOLLECITO_MS - (Date.now() - prontoDalMs);
    if (mancano <= 0) {
      setAdesso(Date.now());
      return;
    }
    const attesa = window.setTimeout(() => setAdesso(Date.now()), mancano + 1_000);
    return () => window.clearTimeout(attesa);
  }, [prontoDalMs]);
  const prontaDaTempo = attesaLunga(prontoDalMs, adesso);

  /**
   * Qualcuno ha premuto «Aggiorna» in questa sessione della scheda.
   *
   * Serve a distinguere due guasti che arrivano dallo stesso campo. Un
   * controllo periodico che non trova la rete finisce anche lui in
   * `stato.errore`, e mostrarlo qui vorrebbe dire una scheda rossa ogni volta
   * che il wifi cade — per una cosa che si ritenta da sola fra mezz'ora e che
   * nessuno deve fare. Uno scaricamento fallito è l'opposto: è la risposta a un
   * gesto, e chi lo ha fatto sta guardando.
   */
  const [tentato, setTentato] = useState(false);
  // Il tentativo è riuscito: il pacchetto è scaricato e verificato. Quel che il
  // nucleo scriverà da qui in poi — il controllo periodico che non trova la rete —
  // non è l'esito di un gesto di chi guarda, e un nuovo tentativo si segna da sé
  // (`installa`). Lasciarlo alzato faceva tornare l'errore rosso sulla scheda
  // «Disponibile» se il pacchetto veniva buttato (S5-1).
  useEffect(() => {
    if (fase === "pronto") setTentato(false);
  }, [fase]);

  const installa = useCallback(
    (quando: QuandoInstallare) => {
      setTentato(true);
      segnaAvanzamento({ tipo: "installa" });
      ipc.aggiornamentiInstalla(quando).catch(onErrore);
    },
    [onErrore],
  );

  /**
   * Ferma lo scaricamento, o butta il pacchetto che aspettava. Non è un errore:
   * la scheda torna com'era, e lo stato arriva da `aggiornamenti:stato`.
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

  // Dopo tre giorni di attesa «Non ora» non regge più: un pacchetto che aspetta
  // la chiusura di un programma che non si chiude mai non può restare muto.
  const chiusaPerOra =
    disponibile !== null && disponibile.versione === nonOra && !prontaDaTempo;
  const nascosta = disponibile === null || disponibile.saltata || chiusaPerOra;
  /**
   * Sul telefono, l'APK di questa versione non si installa sopra questa copia:
   * è firmato con un'altra chiave. «Aggiorna» non c'è — scaricherebbe decine di
   * megabyte che Android rifiuta — e al suo posto si dice come passare alla
   * versione pubblica senza perdere la libreria. Vedi `altraChiave`.
   */
  const altraChiave = disponibile?.altraChiave === true;
  const inCorso = fase !== null && fase !== "pronto";
  const guasto = guastoDaMostrare(tentato, fase, stato?.errore);
  const pronto = fase === "pronto";

  /*
   * Quel che si dice a chi ascolta lo schermo, da una regione sempre montata
   * (`Annuncio.tsx`): la versione nuova, il pacchetto pronto, e lo scaricamento
   * che non è riuscito. L'avanzamento non si annuncia: cambia a ogni pacchetto,
   * e lo dice la barra a chi la chiede.
   */
  const annuncio = (
    <Annuncio
      testo={
        nascosta
          ? null
          : guasto !== null
            ? testoErrore(guasto)
            : pronto
              ? fraseDellaFase("pronto", prontaDaTempo)
              : t("update.available", { v: disponibile.versione })
      }
    />
  );
  if (nascosta) return <>{annuncio}</>;

  const passi = passiDi(fase, FORMA);
  const iconaDi = fase === null ? "disponibile" : fase;
  // `totale` manca quando il server non manda `Content-Length`. Una barra che
  // non sa dove arriva è una barra che mente: in quel caso si mostrano i
  // megabyte scesi e basta, e la barra scorre senza dire quanto.
  const quota = fase === "scarico" ? (avanzamento?.quota ?? null) : null;
  const barra = inCorso;
  // Verifico, installo e riavvio non hanno una percentuale: la barra è piena
  // ma ferma, e il passo corrente lo dice la fila.
  const barraPiena = fase !== "scarico";
  const titolo =
    pronto && !MOBILE
      ? t("update.ready.title", { v: disponibile.versione })
      : t("update.available", { v: disponibile.versione });

  return (
    <>
      {annuncio}
      <Presenza parte="toast-card">
        <div
          className="notizia aggiornamento toast-card"
          data-fase={fase ?? "disponibile"}
          {...(pronto && prontaDaTempo ? { "data-sollecito": "" } : {})}
        >
          <div
            className="icona-fase"
            // Respira solo mentre il lavoro è in corso: a pacchetto fermo, o
            // prima di cominciare, l'icona sta ferma.
            {...(inCorso ? { "data-attiva": "" } : {})}
          >
            <Icona nome={ICONA_DELLA_FASE[iconaDi]} dim={16} />
          </div>
          <div className="corpo-aggiornamento">
            <strong>{titolo}</strong>
            {disponibile.note !== null && !inCorso && (
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
            {passi.length > 0 && (
              <ol className="passi-aggiornamento" aria-label={t("update.steps")}>
                {passi.map((passo) => (
                  <li
                    key={passo.chiave}
                    className="passo"
                    data-stato={passo.stato}
                    {...(passo.stato === "corrente" ? { "aria-current": "step" as const } : {})}
                  >
                    {nomeDelPasso(passo.chiave)}
                  </li>
                ))}
              </ol>
            )}
            {barra && (
              <span className="scaricamento">
                <span
                  className="toast-progress"
                  role="progressbar"
                  aria-label={t("update.progress")}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  {...(quota === null
                    ? { "aria-valuetext": fraseDellaFase(fase, false) }
                    : { "aria-valuenow": Math.round(quota) })}
                >
                  <span
                    
                    style={
                      barraPiena
                        ? { width: "100%" }
                        : quota === null
                          ? undefined
                          : { width: `${quota}%` }
                    }
                  />
                </span>
                {fase === "scarico" && (
                  <span className="quanto">
                    {avanzamento === null
                      ? t("update.starting")
                      : [
                          avanzamento.totale === null
                            ? peso(avanzamento.scaricati)
                            : `${peso(avanzamento.scaricati)} / ${peso(avanzamento.totale)}`,
                          avanzamento.velocita === null
                            ? null
                            : t("update.speed", { v: peso(avanzamento.velocita) }),
                          avanzamento.restanti === null
                            ? null
                            : t(
                                quantoManca(avanzamento.restanti).unita === "s"
                                  ? "update.eta.s"
                                  : "update.eta.min",
                                { n: quantoManca(avanzamento.restanti).n },
                              ),
                        ]
                          .filter((pezzo) => pezzo !== null)
                          .join(" · ")}
                  </span>
                )}
              </span>
            )}
            {fase !== null && (
              <span className="stato-fase">{fraseDellaFase(fase, prontaDaTempo)}</span>
            )}
            {pronto && !MOBILE && !prontaDaTempo && (
              <span className="stato-fase">{t("update.ready.tray")}</span>
            )}
            {guasto !== null && <span className="guasto">{testoErrore(guasto)}</span>}
            <div className="azioni-aggiornamento">
              {inCorso && fase === "scarico" && (
                <button
                  type="button"
                  className="bottone minuto btn-ghost"
                  title={t("update.cancel.hint")}
                  onClick={annulla}
                >
                  {t("update.cancel")}
                </button>
              )}
              {pronto && (
                <>
                  <button
                    type="button"
                    className={
                      prontaDaTempo || MOBILE
                        ? "bottone minuto btn-accent"
                        : "bottone minuto btn-ghost"
                    }
                    onClick={() => installa("subito")}
                  >
                    {MOBILE ? t("update.ready.install") : t("update.ready.now")}
                  </button>
                  <button
                    type="button"
                    className="bottone minuto btn-ghost"
                    title={t("update.ready.cancel.hint")}
                    onClick={annulla}
                  >
                    {t("update.ready.cancel")}
                  </button>
                </>
              )}
              {fase === null && (
                <>
                  {!altraChiave && (
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      onClick={() => installa("subito")}
                    >
                      {t("update.install")}
                    </button>
                  )}
                  {!altraChiave && !MOBILE && (
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("update.atClose.hint")}
                      onClick={() => installa("allaChiusura")}
                    >
                      {t("update.atClose")}
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
                </>
              )}
            </div>
          </div>
          {!inCorso && (
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("update.later")}
              title={t("update.later.hint")}
              onClick={() => setNonOra(disponibile.versione)}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          )}
        </div>
      </Presenza>
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
            ? // Sul telefono il negozio è Google Play (la distribuzione `play`).
              t(MOBILE ? "settings.update.play" : "settings.update.store")
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
          onOnline={() => {
            ipc.apriDocumento(letto.quale).catch(onErrore);
          }}
          onChiudi={() => setLetto(null)}
        />
      )}
    </>
  );
}
