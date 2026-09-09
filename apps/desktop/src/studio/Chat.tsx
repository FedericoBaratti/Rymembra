/**
 * Studio · Chat — chiedere una modifica invece di scriverla.
 *
 * # Perché lo Studio è il posto giusto per un modello
 *
 * Perché ha già le tre cose che a un modello servono e che quasi nessun editor
 * ha tutte insieme: un vocabolario **chiuso e dichiarato** (`studio_registro`),
 * una sorgente di verità che è **testo** modificabile per percorsi
 * (`patch.ts`), e un validatore che **non fallisce mai** e che dice cosa non va
 * con parole riutilizzabili (`studio_valida`, con `forse[]`). Le prime due
 * fanno sì che una proposta si possa applicare; la terza fa sì che una proposta
 * sbagliata si possa correggere invece di essere buttata.
 *
 * # Le due modalità, e perché sono due
 *
 * *Proposta*: il modello propone, qui si vede il confronto, e chi guarda
 * decide. È la modalità in cui si impara cosa fa una richiesta.
 *
 * *Agent*: il modello applica, rilegge la validazione del candidato, corregge e
 * riprova — al massimo quattro giri. È la modalità in cui si chiede qualcosa di
 * grosso e si va a prendere un caffè.
 *
 * Sono due tagli dello stesso gesto — «chi preme applica» — ed è per questo che
 * l'interruttore è un `Segmentato` e non un `Interruttore`: un interruttore
 * dice acceso/spento, e qui non c'è niente di spento.
 *
 * All'apertura è *agent*, e non è la scelta prudente: è quella che risponde
 * alla domanda per cui il pannello esiste, cioè «cambia questa cosa». Aprire in
 * *proposta* metterebbe un passo in mezzo a ogni modifica per proteggere da un
 * gesto che è già reversibile due volte: Ctrl+Z toglie il giro appena
 * applicato, e l'istantanea presa prima del primo riporta al documento di
 * partenza qualunque cosa siano stati i quattro. Chi vuole vedere prima di
 * applicare sposta l'interruttore, e da lì in poi vede.
 *
 * # Ogni modifica passa da `setSorgente`
 *
 * Cioè da `scriviSorgente` di `storia.ts`, cioè dallo stesso annullo di tutto
 * il resto. Una risposta applicata per sbaglio si toglie con Ctrl+Z come si
 * toglie un cursore trascinato male, e in modalità agent si prende
 * un'istantanea **prima** del primo giro: quattro giri sono quattro modifiche,
 * e annullarle una per una sarebbe la punizione sbagliata per aver provato.
 *
 * # La validazione del candidato si chiede a parte
 *
 * Non si aspetta quella dello Studio: quella ha un respiro di 120 millisecondi
 * e riscrive `esito` da sola, e un ciclo che l'aspettasse starebbe in corsa con
 * un timer. `studio_valida` è deterministico e non fallisce mai — chiederlo due
 * volte non costa niente e toglie di mezzo la corsa.
 *
 * # Gli avvisi non fanno ripartire un giro
 *
 * Nello Studio un avviso non blocca l'esportazione, e non deve bloccare nemmeno
 * qui. Un ciclo che si ostinasse sul contrasto girerebbe quattro volte per
 * arrivare dove era già arrivato.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  FineIa,
  GuastoIa,
  MessaggioIa,
  OperazioneIa,
  PezzoIa,
  Registro,
  StatoIa,
  Validazione,
} from "../ipc";
import { ipc } from "../ipc";
import { useAscolto } from "../pagine";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { differenze, quanteCambiano } from "./differenze";
import { applicaOperazioni } from "./patch";
import { correzione, misura, sistema } from "./prompt";
import { t } from "../lingue";

/**
 * Quanti giri può fare la modalità agent.
 *
 * Quattro. Sotto, un modello che sbaglia un nome al primo giro non fa in tempo
 * a correggersi; sopra, un modello che non ci arriva brucia gettoni e minuti
 * ripetendo la stessa cosa — e su un servizio a pagamento quei giri si pagano.
 */
const GIRI_MASSIMI = 4;

/**
 * Quanti caratteri di ragionamento si mostrano mentre arriva.
 *
 * La coda, non la testa: quel che serve mentre si aspetta è la prova che il
 * modello sta ancora scrivendo, e la prova è l'ultima riga, non la prima. Senza
 * un tetto il pannello si riempirebbe di un testo che nessuno legge e che
 * spingerebbe fuori vista il campo in cui si scrive.
 */
const PENSIERO_IN_VISTA = 400;

/** Le due modalità. */
type Modalita = "proposta" | "agent";

/** Una battuta come la mostra il pannello. */
type Battuta = {
  /** Chi ha parlato. Il messaggio di sistema non è mai una battuta. */
  mio: boolean;
  testo: string;
  /**
   * Il ragionamento che ha preceduto questa risposta, se il modello lo mostra.
   *
   * Si tiene per una ragione sola: quando la risposta arriva vuota — succede,
   * su un modello che ragiona fino a esaurire il tetto dei gettoni — è l'unica
   * cosa che spiega dove sia finito il minuto. Sta dentro un `details` chiuso,
   * perché la lunghezza tipica di un ragionamento è quella di dieci risposte.
   */
  pensiero?: string;
};

/**
 * Quel che il modello ha proposto.
 *
 * Le operazioni e non il testo che ne uscirebbe: il candidato si ricalcola dalla
 * sorgente **corrente** a ogni disegno. Tenerlo qui dentro lo congelerebbe al
 * momento della risposta, e chi modifica il documento mentre la proposta è a
 * schermo si vedrebbe rimettere indietro il proprio lavoro premendo «Applica».
 */
type Proposta = {
  operazioni: OperazioneIa[];
  /** Perché qualcosa è stato scartato, dal lettore o dall'applicatore. */
  ragioni: string[];
};

export function Chat({
  sorgente,
  onSorgente,
  registro,
  esito,
  onIstantanea,
  onErrore,
  onChiudi,
  onVaiAImpostazioni,
}: {
  sorgente: string;
  /** `scriviSorgente` dello Studio: ogni applicazione finisce nell'annullo. */
  onSorgente: (testo: string) => void;
  registro: Registro | null;
  esito: Validazione | null;
  /**
   * Salva un'istantanea prima che l'agent cominci.
   *
   * Quattro giri sono quattro modifiche, e annullarle una per una sarebbe la
   * punizione sbagliata per aver provato.
   */
  onIstantanea: () => void;
  onErrore: (e: unknown) => void;
  onChiudi: () => void;
  /** Non c'è nessun modello: la strada per configurarne uno. */
  onVaiAImpostazioni: () => void;
}) {
  const [stato, setStato] = useState<StatoIa | null>(null);
  const [battute, setBattute] = useState<Battuta[]>([]);
  const [scritto, setScritto] = useState("");
  const [modalita, setModalita] = useState<Modalita>("agent");
  const [proposta, setProposta] = useState<Proposta | null>(null);
  /** Il turno in volo, o `null`. */
  const [turno, setTurno] = useState<number | null>(null);
  /** Quel che sta arrivando adesso, prima che il turno finisca. */
  const [parziale, setParziale] = useState("");
  /** Il ragionamento che sta arrivando adesso, per chi lo manda. */
  const [pensiero, setPensiero] = useState("");
  const [giro, setGiro] = useState(0);

  /**
   * La conversazione com'è adesso, per il filo dell'agent.
   *
   * Un riferimento e non lo stato: il ciclo di correzione gira dentro una
   * promessa, e leggere lo stato da lì darebbe quello del disegno in cui il
   * ciclo è partito — cioè, al secondo giro, una conversazione senza il primo.
   */
  const conversazione = useRef<MessaggioIa[]>([]);
  /**
   * Lo stesso testo di `parziale`, ma leggibile subito.
   *
   * `ia:fine` arriva un istante dopo l'ultimo `ia:pezzo`, e i due sono due
   * eventi distinti: se React non ha ancora ridisegnato in mezzo, `parziale`
   * dentro il gestore della fine è quello di **prima** dell'ultimo pezzo. Il
   * pezzo che si perderebbe è l'ultimo, cioè quasi sempre la coda del blocco
   * delle modifiche — un `aether-patch` troncato di due caratteri, che il
   * parser rifiuta senza che niente lo spieghi.
   */
  const arrivato = useRef("");
  /** Come [`arrivato`], per il ragionamento: `ia:fine` lo legge da qui. */
  const pensato = useRef("");
  /**
   * Il documento che l'ultimo giro dell'agent ha prodotto.
   *
   * Serve a riconoscere un modello che si è impuntato: due giri che producono
   * lo stesso identico documento sono due richieste pagate per lo stesso
   * risultato, e il terzo e il quarto sarebbero altre due. La validazione da
   * sola non basta a fermarlo — gli errori che restano sono gli stessi, quindi
   * la condizione «restano errori» resta vera per sempre.
   */
  const ultimoCandidato = useRef<string | null>(null);
  /**
   * Una richiesta è partita e non è ancora finita.
   *
   * Serve a una corsa sola: il filo che genera comincia a emettere appena parte,
   * e la risposta di `ia_conversa` — cioè il numero del turno — torna per una
   * strada diversa. In pratica il primo gettone costa un giro di rete e arriva
   * molto dopo; ma «in pratica» non è una garanzia, e i pezzi arrivati prima del
   * numero cadrebbero nel vuoto. Finché questa bandiera è alzata **una sola**
   * conversazione può esistere — lo impone il nucleo — quindi un evento che
   * arriva adesso è per costruzione il nostro, e il suo numero si adotta.
   */
  const inVolo = useRef(false);
  const fondo = useRef<HTMLDivElement>(null);

  /**
   * Questo evento è del turno che stiamo aspettando.
   *
   * Adotta il numero quando non lo si conosce ancora: vedi [`inVolo`].
   */
  const mio = (numero: number): boolean => {
    if (turno !== null) return numero === turno;
    if (!inVolo.current) return false;
    setTurno(numero);
    return true;
  };

  useEffect(() => {
    ipc.iaProfili().then(setStato).catch(onErrore);
  }, [onErrore]);

  /** Il pannello segue quel che arriva, senza rubare il fuoco al campo. */
  useEffect(() => {
    fondo.current?.scrollIntoView({ block: "end" });
  }, [battute, parziale]);

  /**
   * Chiudere il pannello ferma quel che sta arrivando.
   *
   * Senza, una generazione continuerebbe verso una finestra che non l'ascolta
   * più — a spese di chi la paga, se il modello è remoto — e il posto resterebbe
   * occupato: riaprendo la chat, la prima domanda riceverebbe `ia.busy` per una
   * risposta che nessuno leggerà. Il turno si legge da un riferimento perché
   * questa pulizia gira **dopo** l'ultimo disegno, e lo stato di allora è quello
   * che serve.
   */
  const ultimoTurno = useRef<number | null>(null);
  ultimoTurno.current = turno;
  useEffect(
    () => () => {
      const acceso = ultimoTurno.current;
      if (acceso !== null) void ipc.iaFerma(acceso).catch(() => {});
    },
    [],
  );

  const profilo = useMemo(
    () => stato?.profili.find((p) => p.id === stato.attivo) ?? null,
    [stato],
  );

  // ── il turno ──────────────────────────────────────────────────────────────

  /**
   * Manda quel che c'è nella conversazione e apre un turno.
   *
   * # Perché il documento è un parametro e non `sorgente`
   *
   * Perché il messaggio di sistema porta dentro il documento, e in modalità
   * agent il documento cambia fra un giro e l'altro. Leggerlo dalla prop
   * significherebbe leggere quello del disegno in cui il ciclo è partito: al
   * secondo giro il modello riceverebbe la descrizione della skin **prima**
   * della sua stessa modifica, e correggerebbe qualcosa che non esiste più.
   * Chi chiama ha in mano il documento giusto — è quello che ha appena
   * applicato — e lo passa.
   */
  const manda = useCallback(
    (aggiunta: MessaggioIa, documento: string, quelloCheNonVa: Validazione | null) => {
      if (registro === null) return;
      conversazione.current = [...conversazione.current, aggiunta];
      const messaggi = [
        sistema(registro, documento, quelloCheNonVa),
        ...conversazione.current,
      ];
      arrivato.current = "";
      pensato.current = "";
      setParziale("");
      setPensiero("");
      // Alzata **prima** dell'invocazione, e non nel `then`: è lì che serve,
      // perché è fra qui e il `then` che i primi pezzi potrebbero arrivare.
      inVolo.current = true;
      ipc
        .iaConversa(messaggi)
        .then(setTurno)
        .catch((e: unknown) => {
          inVolo.current = false;
          setTurno(null);
          setGiro(0);
          onErrore(e);
        });
    },
    [registro, onErrore],
  );

  /**
   * Cosa fare del testo appena arrivato, secondo la modalità.
   *
   * In *proposta* si prepara il candidato e ci si ferma. In *agent* si applica,
   * si rilegge la validazione **del candidato**, e se restano errori si
   * rimandano indietro — finché non sono zero, finché i giri finiscono, o
   * finché qualcuno preme «Ferma».
   */
  const concludi = useCallback(
    async (testo: string, tagliato: boolean) => {
      try {
        const lette = await ipc.iaOperazioni(testo);
        // Le due metà della stessa domanda — «cosa ha chiesto» e «cosa si è
        // potuto fare» — e le loro ragioni finiscono in un elenco solo: chi
        // legge non ha motivo di sapere quale delle due l'ha scritta, e il
        // giro di correzione le manda indietro tutte insieme.
        const { testo: candidato, scartate } = applicaOperazioni(
          sorgente,
          lette.operazioni,
        );
        const ragioni = [...lette.ragioni, ...scartate];

        // Il documento non è cambiato, e i casi sono due.
        //
        // Senza ragioni, il modello non ha proposto niente: una risposta che
        // spiega, un rifiuto motivato. Non c'è niente da correggere, e insistere
        // vorrebbe dire chiedere di nuovo la stessa cosa.
        //
        // Con le ragioni, invece, ci ha provato e non è passato niente — ed è
        // **il** caso in cui un giro serve davvero: le ragioni sono l'unica
        // cosa che gli manca. Un modello che ha inventato «insert» impara da
        // qui che si scrive alla posizione, e al giro dopo lo fa. Il documento
        // che riceve è lo stesso di prima, perché nessuna modifica è passata.
        if (candidato === sorgente) {
          setProposta(ragioni.length > 0 ? { operazioni: [], ragioni } : null);
          if (
            modalita !== "agent" ||
            ragioni.length === 0 ||
            tagliato ||
            giro >= GIRI_MASSIMI
          ) {
            setGiro(0);
            return;
          }
          setGiro((prima) => prima + 1);
          manda(
            { ruolo: "user", testo: correzione([], ragioni, false) },
            sorgente,
            esito,
          );
          return;
        }

        if (modalita === "proposta") {
          setProposta({ operazioni: lette.operazioni, ragioni });
          return;
        }

        onSorgente(candidato);
        // Quel che è stato scartato si vede anche qui, dove la modifica è già
        // passata: senza, «nove buone e una storta valgono nove» varrebbe solo
        // in proposta, e in agent la storta sparirebbe — applicata la parte
        // buona, di quella rifiutata non resterebbe traccia da nessuna parte.
        setProposta(ragioni.length > 0 ? { operazioni: [], ragioni } : null);
        const controllo = await ipc.studioValida(candidato);
        const restano = controllo.errori;
        // Un modello che riscrive ogni giro la stessa modifica sbagliata: la
        // validazione da sola non lo ferma, perché gli errori che restano sono
        // sempre gli stessi. Vedi [`ultimoCandidato`].
        const impuntato = candidato === ultimoCandidato.current;
        ultimoCandidato.current = candidato;
        // Gli avvisi non contano — nello Studio non bloccano l'esportazione — e
        // un blocco tagliato non si ripete: chiedere di rifare una risposta
        // troncata darebbe una seconda risposta troncata allo stesso punto.
        if (
          restano.length === 0 ||
          tagliato ||
          impuntato ||
          giro >= GIRI_MASSIMI
        ) {
          setGiro(0);
          return;
        }
        setGiro((prima) => prima + 1);
        manda(
          { ruolo: "user", testo: correzione(restano, ragioni) },
          candidato,
          controllo,
        );
      } catch (e: unknown) {
        setGiro(0);
        onErrore(e);
      }
    },
    [sorgente, modalita, giro, esito, manda, onSorgente, onErrore],
  );

  const chiedi = useCallback(() => {
    const testo = scritto.trim();
    if (testo === "" || turno !== null || registro === null) return;
    setScritto("");
    setProposta(null);
    // Una domanda nuova non eredita il candidato della precedente: due
    // richieste diverse che arrivano allo stesso documento sono una
    // coincidenza, non un modello impuntato.
    ultimoCandidato.current = null;
    setBattute((prima) => [...prima, { mio: true, testo }]);
    if (modalita === "agent") {
      // L'istantanea **prima** del primo giro, non a ogni giro: quel che si
      // vuole poter ritrovare è il documento com'era prima di chiedere.
      onIstantanea();
      setGiro(1);
    }
    manda({ ruolo: "user", testo }, sorgente, esito);
  }, [scritto, turno, registro, modalita, manda, onIstantanea, sorgente, esito]);

  const ferma = useCallback(() => {
    if (turno === null) return;
    // Il giro si azzera **prima** della risposta: il turno può ancora
    // consegnare l'ultimo blocco, e senza questo l'agent ne aprirebbe un altro.
    setGiro(0);
    ipc.iaFerma(turno).catch(onErrore);
  }, [turno, onErrore]);

  useAscolto<PezzoIa>("ia:pezzo", (pezzo) => {
    if (!mio(pezzo.turno)) return;
    // Due cesti e non uno: il ragionamento si guarda, la risposta si legge e si
    // interpreta. Mescolarli darebbe un blocco `aether-patch` con dentro i
    // tentativi che il modello ha scartato mentre pensava.
    if (pezzo.pensiero) {
      pensato.current += pezzo.testo;
      setPensiero(pensato.current);
      return;
    }
    arrivato.current += pezzo.testo;
    setParziale(arrivato.current);
  });

  useAscolto<GuastoIa>("ia:errore", (guasto) => {
    if (!mio(guasto.turno)) return;
    inVolo.current = false;
    arrivato.current = "";
    pensato.current = "";
    setTurno(null);
    setGiro(0);
    setParziale("");
    setPensiero("");
    onErrore(guasto.errore);
  });

  /**
   * Il turno è finito: si legge quel che ha scritto, e si decide se basta.
   *
   * L'estrazione delle operazioni la fa il nucleo (`ia_operazioni`): è la
   * funzione che riceve testo scritto da un modello, ed è l'unica delle due
   * metà che abbia delle prove.
   */
  useAscolto<FineIa>("ia:fine", (fine) => {
    if (!mio(fine.turno)) return;
    // Dal riferimento e non dallo stato: vedi [`arrivato`].
    const testo = arrivato.current;
    const ragionato = pensato.current;
    inVolo.current = false;
    arrivato.current = "";
    pensato.current = "";
    setTurno(null);
    setParziale("");
    setPensiero("");
    setBattute((prima) => [
      ...prima,
      ragionato === ""
        ? { mio: false, testo }
        : { mio: false, testo, pensiero: ragionato },
    ]);
    conversazione.current = [
      ...conversazione.current,
      { ruolo: "assistant", testo },
    ];
    // Fermato a metà: quel che è arrivato si legge, ma non si applica — un
    // blocco troncato è un blocco che non dice cosa manca.
    if (fine.motivo === "fermato") {
      setGiro(0);
      return;
    }
    void concludi(testo, fine.motivo === "tagliato");
  });

  // ── quel che si disegna ───────────────────────────────────────────────────

  /**
   * Il documento che si otterrebbe applicando la proposta, adesso.
   *
   * Ricalcolato dalla sorgente corrente e non conservato: vedi [`Proposta`]. È
   * anche quel che «Applica» scrive, quindi il confronto che si guarda e la
   * modifica che si accetta non possono divergere.
   */
  const candidato = useMemo(
    () =>
      proposta === null
        ? null
        : applicaOperazioni(sorgente, proposta.operazioni).testo,
    [proposta, sorgente],
  );
  const confronto = useMemo(
    () =>
      candidato === null || candidato === sorgente
        ? []
        : differenze(sorgente, candidato).filter((r) => r.segno !== "="),
    [candidato, sorgente],
  );
  const conti = useMemo(() => quanteCambiano(confronto), [confronto]);

  /**
   * Quanto peserebbe il prossimo turno, per chi sceglie il modello.
   *
   * `battute` è nelle dipendenze pur non comparendo nel corpo, ed è
   * deliberato: quel che si misura è `conversazione.current`, che è un
   * riferimento e non farebbe ricalcolare niente da solo. Le due cambiano
   * insieme — una battuta in più è un messaggio in più — e questa è la sola
   * lettura di quel riferimento che debba ridisegnare.
   */
  const peso = useMemo(() => {
    if (registro === null) return "";
    return misura([sistema(registro, sorgente, esito), ...conversazione.current]);
  }, [registro, sorgente, esito, battute]);

  if (stato !== null && profilo === null) {
    return (
      <aside className="chat-studio" aria-label={t("studio.chat.title")}>
        <header className="testa-chat">
          <Icona nome="i-chat" dim={16} />
          <strong>{t("studio.chat.title")}</strong>
          <span className="spinta" />
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("studio.chat.close")}
            onClick={onChiudi}
          >
            <Icona nome="i-x" dim={15} />
          </button>
        </header>
        <div className="chat-vuota">
          <p>{t("studio.chat.noModel")}</p>
          <button type="button" className="bottone btn-ghost" onClick={onVaiAImpostazioni}>
            <Icona nome="i-ia" dim={15} />
            {t("studio.chat.configure")}
          </button>
        </div>
      </aside>
    );
  }

  return (
    <aside className="chat-studio" aria-label={t("studio.chat.title")}>
      <header className="testa-chat">
        <Icona nome="i-chat" dim={16} />
        <strong>{t("studio.chat.title")}</strong>
        <span className="spinta" />
        <Segmentato
          classe="minuto"
          etichetta={t("studio.chat.whichMode")}
          scelta={modalita}
          onScegli={setModalita}
          voci={[
            { chiave: "proposta", etichetta: t("studio.chat.mode.propose") },
            { chiave: "agent", etichetta: t("studio.chat.mode.agent") },
          ]}
        />
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("studio.chat.close")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={15} />
        </button>
      </header>

      <p className="nota-chat">
        {profilo === null
          ? ""
          : t("studio.chat.using", { modello: profilo.modello, peso })}
      </p>

      <div className="chat-conversazione">
        {battute.length === 0 && turno === null && (
          <p className="chat-vuota">{t("studio.chat.empty")}</p>
        )}
        {battute.map((b, i) => (
          <div key={i} className={b.mio ? "chat-messaggio mio" : "chat-messaggio"}>
            {b.pensiero !== undefined && (
              <details className="chat-pensiero">
                <summary>{t("studio.chat.reasoning")}</summary>
                {b.pensiero}
              </details>
            )}
            {b.testo === "" ? t("studio.chat.onlyReasoning") : b.testo}
          </div>
        ))}
        {turno !== null && (
          <div className="chat-messaggio">
            {/* La coda del ragionamento mentre arriva: è la prova che il modello
                sta ancora scrivendo, ed è quel che manca a un pannello che tace
                per un minuto prima della prima parola. Sparisce appena la
                risposta comincia, e torna chiuso dentro la battuta alla fine. */}
            {parziale === "" && pensiero !== "" && (
              <div className="chat-pensiero vivo">
                <strong>{t("studio.chat.reasoningNow")}</strong>
                {pensiero.slice(-PENSIERO_IN_VISTA)}
              </div>
            )}
            {parziale === "" && pensiero === ""
              ? t("studio.chat.thinking")
              : parziale}
          </div>
        )}

        {proposta !== null && (
          <div className="chat-proposta">
            {/* Il confronto e non il numero di operazioni: sono le righe che
                cambiano a dire se c'è qualcosa da applicare. Un elenco di
                modifiche che lascia il documento com'era — il modello ha
                riscritto valori che c'erano già — mostrava «N modifiche
                proposte» sopra un riquadro vuoto e un bottone che non faceva
                niente. */}
            {confronto.length > 0 ? (
              <>
                <div className="riassunto">
                  {t("studio.chat.proposed", {
                    n: proposta.operazioni.length,
                    piu: conti.aggiunte,
                    meno: conti.tolte,
                  })}
                </div>
                <pre className="differenze">
                  {confronto.map((r, i) => (
                    <div key={i} className={r.segno === "+" ? "d-piu" : "d-meno"}>
                      <span className="segno">{r.segno}</span>
                      <code>{r.testo}</code>
                    </div>
                  ))}
                </pre>
              </>
            ) : proposta.ragioni.length === 0 ? (
              <div className="riassunto">
                {proposta.operazioni.length > 0
                  ? t("studio.chat.noChange")
                  : t("studio.chat.nothingToApply")}
              </div>
            ) : null}
            {/* Con le ragioni sopra non ci va nessun riassunto: dicono già loro
                cosa non è passato, e una riga «niente da applicare» sopra un
                elenco di rifiuti direbbe la stessa cosa peggio. */}
            {proposta.ragioni.length > 0 && (
              <ul className="scartate">
                {proposta.ragioni.map((r, i) => (
                  <li key={i}>{r}</li>
                ))}
              </ul>
            )}
            {confronto.length > 0 && candidato !== null && (
              <div className="azioni">
                <button
                  type="button"
                  className="bottone primario btn-accent"
                  onClick={() => {
                    onSorgente(candidato);
                    setProposta(null);
                  }}
                >
                  <Icona nome="i-check" dim={15} />
                  {t("studio.chat.apply")}
                </button>
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={() => setProposta(null)}
                >
                  {t("studio.chat.discard")}
                </button>
              </div>
            )}
          </div>
        )}
        <div ref={fondo} />
      </div>

      <div className="chat-scrivi">
        <textarea
          className="campo field-input"
          rows={3}
          placeholder={t("studio.chat.placeholder")}
          value={scritto}
          disabled={turno !== null}
          onChange={(e) => setScritto(e.target.value)}
          onKeyDown={(e) => {
            // Invio manda, Maiusc+Invio va a capo: è la convenzione di ogni
            // chat, e in un campo da tre righe l'a capo serve di rado.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              chiedi();
            }
          }}
        />
        <div className="azioni">
          {turno === null ? (
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={scritto.trim() === "" || registro === null}
              onClick={chiedi}
            >
              <Icona nome="i-chat" dim={15} />
              {t("studio.chat.send")}
            </button>
          ) : (
            <button type="button" className="bottone btn-ghost" onClick={ferma}>
              <Icona nome="i-x" dim={15} />
              {t("studio.chat.stop")}
            </button>
          )}
          {giro > 0 && (
            <span className="giro">
              {t("studio.chat.round", { n: giro, di: GIRI_MASSIMI })}
            </span>
          )}
        </div>
      </div>
    </aside>
  );
}
