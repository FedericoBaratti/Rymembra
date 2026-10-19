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
 * # Quel che parte con la domanda
 *
 * La selezione dello Studio, in una riga davanti al testo: «questo» vuol dire
 * la parte scelta, con lo stato e il tema che l'ispettore sta guardando. Il
 * messaggio di sistema porta il documento **leggero** — le sezioni che la
 * domanda, la selezione e gli errori nominano, e l'elenco di quel che si è
 * lasciato fuori — e la conversazione vecchia arriva riassunta oltre una
 * soglia (`prompt.ts`). A richiesta, anche una fotografia della tela
 * (`fotografia.ts`): solo dati finti, e solo per la domanda che la porta.
 *
 * # La conversazione resta
 *
 * In `skin/bozze/<id>/chat.json`, accanto alla bozza di cui parla: chiudere
 * il cassetto o lo Studio non la perde, «Scarta» la porta via con la bozza.
 * Le immagini non si salvano (`conversazione.ts`).
 *
 * # Ogni modifica passa da `onSorgente`
 *
 * Cioè dallo stesso annullo di tutto il resto. Una risposta applicata per
 * sbaglio si toglie con Ctrl+Z come si toglie un cursore trascinato male, e in
 * modalità agent si prende un'istantanea **prima** del primo giro: quattro
 * giri sono quattro modifiche, e annullarle una per una sarebbe la punizione
 * sbagliata per aver provato. Ctrl+Z dentro il campo della chat è del campo:
 * lo Studio non lo prende (`Guscio.tsx`, `siScrive`).
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
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";

import type {
  FineIa,
  GuastoIa,
  MessaggioIa,
  OperazioneIa,
  PezzoIa,
  StatoIa,
  Validazione,
} from "../ipc";
import { ipc } from "../ipc";
import { locale, t } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { useStudio } from "./contesto";
import {
  leggiSalvata,
  mostraCambi,
  serializza,
  type Battuta,
  type CambioMostrato,
  type Conversazione,
  type MessaggioChat,
  type Modalita,
} from "./conversazione";
import { differenze, differenzeFoglia, quanteCambiano, type Cambio } from "./differenze";
import { fotografaTela, telaDelloStudio } from "./fotografia";
import { applicaOperazioni } from "./patch";
import {
  BLOCCO,
  correzione,
  haBloccoDiModifiche,
  rigaSelezione,
  sistema,
  stimaGettoni,
  storiaLeggera,
  type Sguardo,
} from "./prompt";
import { useAlloSmontaggio, useAscoltiTenaci } from "./tenace";
import { useVai } from "./vai";

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

/** Quanto aspetta il salvataggio della conversazione dopo l'ultima battuta. */
const RESPIRO_SALVATAGGIO = 400;

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

/** Cosa è scelto nello Studio, in parole per chi legge; `null` se niente. */
function etichetta({ selezione, stato, tema }: Sguardo): string | null {
  if (selezione === null) return null;
  let cosa: string;
  let temaConta = false;
  switch (selezione.tipo) {
    case "documento":
      return null;
    case "parte":
      cosa =
        stato === null
          ? t("studio.chat.sel.part", { nome: selezione.nome })
          : t("studio.chat.sel.partState", { nome: selezione.nome, stato });
      temaConta = true;
      break;
    case "token":
      cosa = t("studio.chat.sel.token", { nome: selezione.id });
      temaConta = true;
      break;
    case "colore":
      cosa = t("studio.chat.sel.color", { nome: selezione.nome });
      temaConta = true;
      break;
    case "motivo":
      cosa = t("studio.chat.sel.pattern", { nome: selezione.nome });
      temaConta = true;
      break;
    case "curva":
      cosa = t("studio.chat.sel.easing", { nome: selezione.nome });
      break;
    case "animazione":
      cosa = t("studio.chat.sel.animation", { nome: selezione.nome });
      break;
    case "carattere":
      cosa = t("studio.chat.sel.font", { nome: selezione.nome });
      break;
    case "risorsa":
      cosa = t("studio.chat.sel.file", { nome: selezione.nome });
      break;
    case "nodo":
      cosa = t("studio.chat.sel.node");
      break;
  }
  return temaConta && tema === "variante" ? t("studio.chat.sel.variant", { cosa }) : cosa;
}

/** Quel che cambia fra due testi, per percorso; `null` se uno dei due non è JSON. */
function cambiFra(prima: string, dopo: string): Cambio[] | null {
  try {
    return differenzeFoglia(JSON.parse(prima), JSON.parse(dopo));
  } catch {
    return null;
  }
}

/** Aggiunge qualcosa all'ultima risposta del modello. */
function segnaUltima(battute: readonly Battuta[], aggiunta: Partial<Battuta>): Battuta[] {
  for (let i = battute.length - 1; i >= 0; i--) {
    const b = battute[i];
    if (b === undefined || b.mio) continue;
    return [...battute.slice(0, i), { ...b, ...aggiunta }, ...battute.slice(i + 1)];
  }
  return [...battute];
}

/** Un numero come lo scrive la lingua scelta; `?` se il servizio non l'ha detto. */
function numero(n: number | null): string {
  return n === null ? "?" : n.toLocaleString(locale());
}

/**
 * Una risposta del modello, coi blocchi di modifiche chiusi.
 *
 * Il blocco è per lo Studio, non per chi legge: quel che ha cambiato si legge
 * sotto, per percorso. Resta a un clic per chi vuole vedere cosa ha scritto
 * davvero.
 */
function Risposta({ testo }: { testo: string }) {
  const pezzi: { blocco: boolean; testo: string }[] = [];
  let da = 0;
  for (const trovato of testo.matchAll(BLOCCO)) {
    const dove = trovato.index;
    if (dove > da) pezzi.push({ blocco: false, testo: testo.slice(da, dove) });
    pezzi.push({ blocco: true, testo: trovato[0] });
    da = dove + trovato[0].length;
  }
  if (da < testo.length) pezzi.push({ blocco: false, testo: testo.slice(da) });
  return (
    <>
      {pezzi.map((p, i) =>
        p.blocco ? (
          <details key={i} className="chat-blocco">
            <summary>{t("studio.chat.patchBlock")}</summary>
            <pre>{p.testo}</pre>
          </details>
        ) : (
          <span key={i}>{p.testo.replace(/^\n+|\n+$/g, "")}</span>
        ),
      )}
    </>
  );
}

/** I valori cambiati, ognuno col suo «vai». */
function ElencoCambi({
  cambi,
  altri,
  onVai,
}: {
  cambi: readonly CambioMostrato[];
  altri: number;
  onVai: (percorso: string) => void;
}) {
  return (
    <ul className="chat-cambi">
      {cambi.map((c) => (
        <li key={c.percorso}>
          <button
            type="button"
            title={t("studio.chat.go")}
            onClick={() => onVai(c.percorso)}
          >
            <code>{c.percorso === "" ? t("studio.chat.wholeDocument") : c.percorso}</code>
          </button>
          {c.prima !== null && <del>{c.prima}</del>}
          {c.dopo === null ? <span>{t("studio.chat.removed")}</span> : <ins>{c.dopo}</ins>}
        </li>
      ))}
      {altri > 0 && <li>{t("studio.chat.moreChanges", { n: altri })}</li>}
    </ul>
  );
}

export function Chat({
  sorgente,
  onSorgente,
  onIstantanea,
  onErrore,
  onChiudi,
  onVaiAImpostazioni,
}: {
  sorgente: string;
  /** La scrittura dello Studio: ogni applicazione finisce nell'annullo. */
  onSorgente: (testo: string) => void;
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
  const { s, registro } = useStudio();
  const vai = useVai();
  const esito = s.esito;
  const sguardo = useMemo<Sguardo>(
    () => ({ selezione: s.selezione, stato: s.stato, tema: s.tema }),
    [s.selezione, s.stato, s.tema],
  );

  const [stato, setStato] = useState<StatoIa | null>(null);
  /** La conversazione salvata è arrivata: da qui in poi si salva. */
  const [caricata, setCaricata] = useState(false);
  const [battute, setBattute] = useState<Battuta[]>([]);
  const [scritto, setScritto] = useState("");
  const [modalita, setModalita] = useState<Modalita>("agent");
  const [vede, setVede] = useState(false);
  const [proposta, setProposta] = useState<Proposta | null>(null);
  /** Il turno in volo, o `null`. */
  const [turno, setTurno] = useState<number | null>(null);
  /** Si sta fotografando la tela: la domanda parte appena è pronta. */
  const [preparando, setPreparando] = useState(false);
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
  const conversazione = useRef<MessaggioChat[]>([]);
  /**
   * L'ultima domanda di chi scrive. Decide quali sezioni del documento
   * servono — anche nei giri di correzione, dove l'ultimo messaggio è dello
   * Studio e non nomina niente.
   */
  const richiesta = useRef("");
  /** Quel che serve dopo un'attesa: il disegno di allora è vecchio. */
  const attuali = useRef({ sorgente, esito, sguardo });
  attuali.current = { sorgente, esito, sguardo };
  /**
   * Lo stesso testo di `parziale`, ma leggibile subito.
   *
   * `ia:fine` arriva un istante dopo l'ultimo `ia:pezzo`, e i due sono due
   * eventi distinti: se React non ha ancora ridisegnato in mezzo, `parziale`
   * dentro il gestore della fine è quello di **prima** dell'ultimo pezzo. Il
   * pezzo che si perderebbe è l'ultimo, cioè quasi sempre la coda del blocco
   * delle modifiche — un `rymembra-patch` troncato di due caratteri, che il
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
   * Il pannello, per distinguere «nascosto» da «chiuso»: vedi `tenace.ts`.
   * Lo Studio nascosto da «Prova sull'app» lascia la chat dov'è, e la
   * risposta che sta arrivando continua ad arrivare.
   */
  const pannello = useRef<HTMLElement>(null);

  /**
   * Segna l'ultima risposta del modello con l'esito delle sue modifiche.
   *
   * La cronologia che torna al modello lo racconta (`storiaLeggera`): fino
   * alla 3.0 diceva «già applicate» anche delle proposte scartate, e il giro
   * dopo partiva da una modifica che non c'era. Una risposta senza blocco non
   * si segna: non aveva niente da applicare.
   */
  const segnaRisposta = useCallback((applicata: boolean) => {
    const messaggi = conversazione.current;
    for (let i = messaggi.length - 1; i >= 0; i--) {
      const m = messaggi[i];
      if (m === undefined || m.ruolo !== "assistant") continue;
      if (!haBloccoDiModifiche(m.testo)) return;
      conversazione.current = [...messaggi.slice(0, i), { ...m, applicata }, ...messaggi.slice(i + 1)];
      return;
    }
  }, []);

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

  // ── la conversazione salvata ──────────────────────────────────────────────
  //
  // Si legge una volta, con l'id di quando il cassetto si apre; si scrive con
  // l'id di adesso, perché la bozza può cambiare nome mentre si parla — e la
  // rinomina si porta dietro il file.
  const idAttuale = useRef(s.id);
  idAttuale.current = s.id;
  /** L'ultimo testo arrivato sul disco, per non riscrivere lo stesso. */
  const scritta = useRef<string | null>(null);
  /** Le scritture una dietro l'altra: due in volo potrebbero arrivare al contrario. */
  const catena = useRef<Promise<void>>(Promise.resolve());

  //
  // Una volta sola per cassetto aperto. Lo Studio sotto `<Activity>` rifà gli
  // effetti quando torna visibile, e fino alla 3.0 questa lettura ripartiva e
  // scriveva la conversazione del disco **sopra** quella in memoria — più
  // nuova di una battuta, o di un giro intero dell'agent finito mentre si
  // provava la skin sull'app.
  const lettura = useRef(false);
  useEffect(() => {
    if (lettura.current) return;
    lettura.current = true;
    ipc
      .studioChatLeggi(idAttuale.current)
      .then((testo) => {
        const letta = leggiSalvata(testo);
        conversazione.current = letta.messaggi;
        scritta.current = serializza(letta);
        setBattute(letta.battute);
        setModalita(letta.modalita);
        setVede(letta.vede);
        setCaricata(true);
      })
      // Un file che non si legge non si sovrascrive: `caricata` resta falso,
      // e questa conversazione vive solo finché il cassetto è aperto.
      .catch(onErrore);
  }, [onErrore]);

  const salva = useCallback(
    (quale: Conversazione) => {
      const testo = serializza(quale);
      if (testo === scritta.current) return;
      scritta.current = testo;
      catena.current = catena.current
        .then(() => ipc.studioChatScrivi(idAttuale.current, testo))
        .catch(onErrore);
    },
    [onErrore],
  );

  const daSalvare = useRef<Conversazione | null>(null);
  daSalvare.current = caricata
    ? { modalita, vede, battute, messaggi: conversazione.current }
    : null;
  useEffect(() => {
    if (!caricata) return;
    const attesa = window.setTimeout(() => {
      if (daSalvare.current) salva(daSalvare.current);
    }, RESPIRO_SALVATAGGIO);
    return () => window.clearTimeout(attesa);
  }, [caricata, modalita, vede, battute, salva]);
  // Chiudendo il cassetto si scrive subito quel che il respiro non ha fatto
  // in tempo a scrivere.
  useEffect(
    () => () => {
      if (daSalvare.current) salva(daSalvare.current);
    },
    [salva],
  );

  /** Il pannello segue quel che arriva, senza rubare il fuoco al campo. */
  useEffect(() => {
    fondo.current?.scrollIntoView({ block: "end" });
  }, [battute, parziale, proposta]);

  /**
   * Chiudere il pannello ferma quel che sta arrivando.
   *
   * Senza, una generazione continuerebbe verso una finestra che non l'ascolta
   * più — a spese di chi la paga, se il modello è remoto — e il posto resterebbe
   * occupato: riaprendo la chat, la prima domanda riceverebbe `ia.busy` per una
   * risposta che nessuno leggerà. Il turno si legge da un riferimento perché
   * questa pulizia gira **dopo** l'ultimo disegno, e lo stato di allora è quello
   * che serve.
   *
   * Chiudere, non nascondere: lo Studio nascosto da «Prova sull'app» non è
   * una chat chiusa, e fino alla 3.0 provare la skin a metà risposta la
   * buttava via. Vedi `tenace.ts`.
   */
  const ultimoTurno = useRef<number | null>(null);
  ultimoTurno.current = turno;
  useAlloSmontaggio(pannello, () => {
    const acceso = ultimoTurno.current;
    if (acceso !== null) void ipc.iaFerma(acceso).catch(() => {});
  });

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
      conversazione.current = [...conversazione.current, aggiunta];
      const messaggi = [
        sistema({
          registro,
          documento,
          esito: quelloCheNonVa,
          sguardo: attuali.current.sguardo,
          richiesta: richiesta.current,
        }),
        ...storiaLeggera(conversazione.current),
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
          segnaRisposta(false);
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
        segnaRisposta(true);
        // Quel che è cambiato resta scritto sotto la risposta che l'ha fatto,
        // per percorso: è la risposta alla domanda «cosa ha toccato?», e un
        // clic porta alla cosa.
        const cambi = cambiFra(sorgente, candidato);
        if (cambi !== null) {
          const mostrati = mostraCambi(cambi);
          setBattute((prima) =>
            segnaUltima(prima, {
              cambi: mostrati.cambi,
              ...(mostrati.altri > 0 ? { altriCambi: mostrati.altri } : {}),
            }),
          );
        }
        // Quel che è stato scartato si vede anche qui, dove la modifica è già
        // passata: senza, «nove buone e una storta valgono nove» varrebbe solo
        // in proposta, e in agent la storta sparirebbe — applicata la parte
        // buona, di quella rifiutata non resterebbe traccia da nessuna parte.
        setProposta(ragioni.length > 0 ? { operazioni: [], ragioni } : null);
        const controllo = await ipc.studioValida(candidato, idAttuale.current);
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
    [sorgente, modalita, giro, esito, manda, onSorgente, onErrore, segnaRisposta],
  );

  const chiedi = useCallback(async () => {
    const testo = scritto.trim();
    if (testo === "" || turno !== null || preparando || inVolo.current) return;
    setScritto("");
    // Una proposta ancora a schermo e una domanda nuova: la proposta è
    // lasciata cadere, e la cronologia lo deve dire.
    if (proposta !== null && proposta.operazioni.length > 0) segnaRisposta(false);
    setProposta(null);
    // Una domanda nuova non eredita il candidato della precedente: due
    // richieste diverse che arrivano allo stesso documento sono una
    // coincidenza, non un modello impuntato.
    ultimoCandidato.current = null;
    richiesta.current = testo;
    const riga = rigaSelezione(sguardo);
    const cosa = etichetta(sguardo);

    let immagine: string | undefined;
    if (vede) {
      setPreparando(true);
      try {
        const tela = telaDelloStudio();
        if (tela !== null) immagine = await fotografaTela(tela);
      } catch {
        // Senza fotografia la domanda parte lo stesso: si dice, e basta.
        onErrore(new Error(t("studio.chat.previewFailed")));
      }
      setPreparando(false);
    }

    const battuta: Battuta = { mio: true, testo };
    if (cosa !== null) battuta.selezione = cosa;
    if (immagine !== undefined) battuta.immagine = immagine;
    setBattute((prima) => [...prima, battuta]);
    if (modalita === "agent") {
      // L'istantanea **prima** del primo giro, non a ogni giro: quel che si
      // vuole poter ritrovare è il documento com'era prima di chiedere.
      onIstantanea();
      setGiro(1);
    }
    const messaggio: MessaggioIa = {
      ruolo: "user",
      testo: riga === null ? testo : `${riga}\n${testo}`,
    };
    if (immagine !== undefined) messaggio.immagini = [immagine];
    // Dopo la fotografia il disegno di prima è vecchio: documento ed esito
    // si prendono adesso.
    manda(messaggio, attuali.current.sorgente, attuali.current.esito);
  }, [
    scritto,
    turno,
    preparando,
    proposta,
    sguardo,
    vede,
    modalita,
    manda,
    onIstantanea,
    onErrore,
    segnaRisposta,
  ]);

  const ferma = useCallback(() => {
    if (turno === null) return;
    // Il giro si azzera **prima** della risposta: il turno può ancora
    // consegnare l'ultimo blocco, e senza questo l'agent ne aprirebbe un altro.
    setGiro(0);
    ipc.iaFerma(turno).catch(onErrore);
  }, [turno, onErrore]);

  /** Ricomincia da capo: la modalità e la fotografia restano come sono. */
  const nuova = useCallback(() => {
    if (turno !== null || preparando) return;
    conversazione.current = [];
    richiesta.current = "";
    ultimoCandidato.current = null;
    setBattute([]);
    setProposta(null);
    setGiro(0);
  }, [turno, preparando]);

  // Gli eventi del turno, anche mentre lo Studio è nascosto: quel che arriva
  // allora si consegna, in ordine, quando torna visibile. Vedi `tenace.ts`.
  const allArrivo = (pezzo: PezzoIa) => {
    if (!mio(pezzo.turno)) return;
    // Due cesti e non uno: il ragionamento si guarda, la risposta si legge e si
    // interpreta. Mescolarli darebbe un blocco `rymembra-patch` con dentro i
    // tentativi che il modello ha scartato mentre pensava.
    if (pezzo.pensiero) {
      pensato.current += pezzo.testo;
      setPensiero(pensato.current);
      return;
    }
    arrivato.current += pezzo.testo;
    setParziale(arrivato.current);
  };

  const alGuasto = (guasto: GuastoIa) => {
    if (!mio(guasto.turno)) return;
    inVolo.current = false;
    arrivato.current = "";
    pensato.current = "";
    setTurno(null);
    setGiro(0);
    setParziale("");
    setPensiero("");
    onErrore(guasto.errore);
  };

  /**
   * Il turno è finito: si legge quel che ha scritto, e si decide se basta.
   *
   * L'estrazione delle operazioni la fa il nucleo (`ia_operazioni`): è la
   * funzione che riceve testo scritto da un modello, ed è l'unica delle due
   * metà che abbia delle prove.
   */
  const allaFine = (fine: FineIa) => {
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
    const risposta: Battuta = { mio: false, testo };
    if (ragionato !== "") risposta.pensiero = ragionato;
    if (fine.gettoniIn !== null || fine.gettoniOut !== null) {
      risposta.gettoni = { entrata: fine.gettoniIn, uscita: fine.gettoniOut };
    }
    setBattute((prima) => [...prima, risposta]);
    conversazione.current = [
      ...conversazione.current,
      { ruolo: "assistant", testo },
    ];
    // Fermato a metà: quel che è arrivato si legge, ma non si applica — un
    // blocco troncato è un blocco che non dice cosa manca.
    if (fine.motivo === "fermato") {
      // Un blocco troncato non si applica: la cronologia non deve dire il
      // contrario.
      segnaRisposta(false);
      setGiro(0);
      return;
    }
    void concludi(testo, fine.motivo === "tagliato");
  };

  useAscoltiTenaci(pannello, {
    "ia:pezzo": (carico) => allArrivo(carico as PezzoIa),
    "ia:errore": (carico) => alGuasto(carico as GuastoIa),
    "ia:fine": (carico) => allaFine(carico as FineIa),
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
  /**
   * Il confronto per percorso, quando tutti e due i testi sono JSON: dice
   * «`parts.card.radius`: 12 → 16» invece di due righe di testo da
   * confrontare a occhio. Per un documento rotto resta il confronto per riga.
   */
  const cambiProposti = useMemo(
    () => (candidato === null || candidato === sorgente ? null : cambiFra(sorgente, candidato)),
    [candidato, sorgente],
  );
  const confronto = useMemo(
    () =>
      candidato === null || candidato === sorgente || cambiProposti !== null
        ? []
        : differenze(sorgente, candidato).filter((r) => r.segno !== "="),
    [candidato, sorgente, cambiProposti],
  );
  const conti = useMemo(() => quanteCambiano(confronto), [confronto]);
  const cambia = cambiProposti !== null ? cambiProposti.length > 0 : confronto.length > 0;
  const mostrati = useMemo(
    () => (cambiProposti === null ? null : mostraCambi(cambiProposti)),
    [cambiProposti],
  );

  /**
   * Quanto peserebbe il prossimo turno, per chi sceglie il modello: le
   * istruzioni per questa domanda, la storia come partirebbe, e la
   * fotografia se c'è.
   *
   * Sui valori differiti: le istruzioni si ricostruiscono dal documento, e
   * ricostruirle a ogni tasto o a ogni passo di un cursore trascinato
   * rallenterebbe proprio quel che si sta facendo. `battute` sta nelle
   * dipendenze perché `conversazione` è un riferimento: le due cambiano
   * insieme.
   */
  const scrittoLento = useDeferredValue(scritto);
  const sorgenteLenta = useDeferredValue(sorgente);
  const peso = useMemo(() => {
    const prossima: MessaggioIa = { ruolo: "user", testo: scrittoLento };
    if (vede) prossima.immagini = [""];
    const messaggi = [
      sistema({ registro, documento: sorgenteLenta, esito, sguardo, richiesta: scrittoLento }),
      ...storiaLeggera([...conversazione.current, prossima]),
    ];
    const caratteri = messaggi.reduce((somma, m) => somma + m.testo.length, 0);
    return t("studio.chat.size", {
      k: Math.max(1, Math.round(caratteri / 1024)),
      gettoni: numero(stimaGettoni(messaggi)),
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- vedi sopra
  }, [registro, sorgenteLenta, esito, sguardo, scrittoLento, vede, battute]);

  /** I gettoni spesi finora, per quel che i servizi ne hanno detto. */
  const spesi = useMemo(() => {
    let entrata = 0;
    let uscita = 0;
    let noti = false;
    for (const b of battute) {
      if (b.gettoni === undefined) continue;
      noti = true;
      entrata += b.gettoni.entrata ?? 0;
      uscita += b.gettoni.uscita ?? 0;
    }
    return noti ? { entrata, uscita } : null;
  }, [battute]);

  const sceltaAdesso = etichetta(sguardo);
  const occupata = turno !== null || preparando;

  const testa = (
    <header className="testa-chat">
      <Icona nome="i-chat" dim={16} />
      <strong>{t("studio.chat.title")}</strong>
      <span className="spinta" />
      {profilo !== null && (
        <>
          <button
            type="button"
            className="tasto-icona icon-btn"
            aria-label={t("studio.chat.new")}
            title={t("studio.chat.new")}
            disabled={occupata || battute.length === 0}
            onClick={nuova}
          >
            <Icona nome="i-plus" dim={15} />
          </button>
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
        </>
      )}
      <button
        type="button"
        className="tasto-icona icon-btn"
        aria-label={t("studio.chat.close")}
        title={t("studio.chat.close")}
        onClick={onChiudi}
      >
        <Icona nome="i-x" dim={15} />
      </button>
    </header>
  );

  if (stato !== null && profilo === null) {
    return (
      <aside ref={pannello} className="chat-studio" aria-label={t("studio.chat.title")}>
        {testa}
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
    <aside ref={pannello} className="chat-studio" aria-label={t("studio.chat.title")}>
      {testa}

      <p className="nota-chat">
        {profilo === null ? "" : t("studio.chat.using", { modello: profilo.modello, peso })}
        {spesi !== null && (
          <>
            <br />
            {t("studio.chat.spent", {
              entrata: numero(spesi.entrata),
              uscita: numero(spesi.uscita),
            })}
          </>
        )}
      </p>

      <div className="chat-conversazione" aria-live="polite" aria-busy={occupata}>
        {battute.length === 0 && turno === null && (
          <p className="chat-vuota">{t("studio.chat.empty")}</p>
        )}
        {battute.map((b, i) => (
          <div key={i} className={b.mio ? "chat-messaggio mio" : "chat-messaggio"}>
            {b.mio && (b.selezione !== undefined || b.immagine !== undefined) && (
              <div className="chat-segni">
                {b.selezione !== undefined && (
                  <span className="chat-scelta">
                    <Icona nome="i-layers" dim={12} />
                    {b.selezione}
                  </span>
                )}
                {typeof b.immagine === "string" ? (
                  <img className="chat-foto" src={b.immagine} alt={t("studio.chat.withPreview")} />
                ) : b.immagine === true ? (
                  <span className="chat-scelta">
                    <Icona nome="i-eye" dim={12} />
                    {t("studio.chat.withPreview")}
                  </span>
                ) : null}
              </div>
            )}
            {b.pensiero !== undefined && (
              <details className="chat-pensiero">
                <summary>{t("studio.chat.reasoning")}</summary>
                {b.pensiero}
              </details>
            )}
            {b.mio ? (
              b.testo
            ) : b.testo === "" ? (
              t("studio.chat.onlyReasoning")
            ) : (
              <Risposta testo={b.testo} />
            )}
            {b.cambi !== undefined && b.cambi.length > 0 && (
              <div className="chat-applicato">
                <div className="riassunto">
                  {t("studio.chat.changed", { n: b.cambi.length + (b.altriCambi ?? 0) })}
                </div>
                <ElencoCambi cambi={b.cambi} altri={b.altriCambi ?? 0} onVai={vai} />
              </div>
            )}
            {b.gettoni !== undefined && (
              <div className="chat-gettoni">
                {t("studio.chat.tokens", {
                  entrata: numero(b.gettoni.entrata),
                  uscita: numero(b.gettoni.uscita),
                })}
              </div>
            )}
          </div>
        ))}
        {(turno !== null || preparando) && (
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
            {preparando
              ? t("studio.chat.photographing")
              : parziale === "" && pensiero === ""
                ? t("studio.chat.thinking")
                : parziale}
          </div>
        )}

        {proposta !== null && (
          <div className="chat-proposta">
            {/* Il confronto e non il numero di operazioni: sono i valori che
                cambiano a dire se c'è qualcosa da applicare. Un elenco di
                modifiche che lascia il documento com'era — il modello ha
                riscritto valori che c'erano già — mostrava «N modifiche
                proposte» sopra un riquadro vuoto e un bottone che non faceva
                niente. */}
            {cambia && mostrati !== null ? (
              <>
                <div className="riassunto">
                  {t("studio.chat.proposedPaths", { n: cambiProposti?.length ?? 0 })}
                </div>
                <ElencoCambi cambi={mostrati.cambi} altri={mostrati.altri} onVai={vai} />
              </>
            ) : cambia ? (
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
            {cambia && candidato !== null && (
              <div className="azioni">
                <button
                  type="button"
                  className="bottone primario btn-accent"
                  onClick={() => {
                    onSorgente(candidato);
                    segnaRisposta(true);
                    // Applicata, la proposta diventa storia: i suoi cambi
                    // restano sotto la risposta che li ha proposti.
                    if (mostrati !== null) {
                      setBattute((prima) =>
                        segnaUltima(prima, {
                          cambi: mostrati.cambi,
                          ...(mostrati.altri > 0 ? { altriCambi: mostrati.altri } : {}),
                        }),
                      );
                    }
                    setProposta(null);
                  }}
                >
                  <Icona nome="i-check" dim={15} />
                  {t("studio.chat.apply")}
                </button>
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={() => {
                    segnaRisposta(false);
                    setProposta(null);
                  }}
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
        <div className="chat-contesto">
          {sceltaAdesso !== null && (
            <span className="chat-scelta" title={t("studio.chat.selectedHint")}>
              <Icona nome="i-layers" dim={12} />
              {sceltaAdesso}
            </span>
          )}
          <span className="spinta" />
          <button
            type="button"
            className="chat-vede"
            aria-pressed={vede}
            title={t("studio.chat.seeHint")}
            onClick={() => setVede((prima) => !prima)}
          >
            <Icona nome="i-eye" dim={13} />
            {t("studio.chat.see")}
          </button>
        </div>
        <textarea
          className="campo field-input"
          rows={3}
          placeholder={t("studio.chat.placeholder")}
          value={scritto}
          disabled={occupata}
          onChange={(e) => setScritto(e.target.value)}
          onKeyDown={(e) => {
            // Invio manda, Maiusc+Invio va a capo: è la convenzione di ogni
            // chat, e in un campo da tre righe l'a capo serve di rado.
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void chiedi();
            }
          }}
        />
        <div className="azioni">
          {turno === null ? (
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={scritto.trim() === "" || preparando}
              onClick={() => void chiedi()}
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
