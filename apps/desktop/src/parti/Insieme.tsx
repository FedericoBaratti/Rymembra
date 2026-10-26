/**
 * «Ascolta insieme» nella finestra: il pannello, la fascia della sessione, gli
 * inviti, il codice, il firewall, la calibrazione e la scheda delle
 * Impostazioni.
 *
 * # Un archivio solo, fuori da React
 *
 * Lo stato della sessione lo decide il nucleo e arriva intero con
 * `insieme:stato` a ogni cambiamento: la finestra non ne tiene una copia sua,
 * lo disegna. Lo leggono però sette posti — il tasto nella barra, quello nella
 * colonna, la fascia in cima alla pagina, gli inviti, il pannello, la finestra
 * del codice, la scheda delle Impostazioni — e farlo scendere da `App` vorrebbe
 * dire ridisegnare la finestra intera per un vicino che compare in rete. Sta in
 * un archivio (`archivio.ts`), e ognuno si iscrive alla parte che gli serve.
 *
 * L'ascolto degli eventi lo apre **un** componente, montato una volta in `App`
 * fuori dalle copie: `InsiemeInApp`. La fascia e gli inviti invece si montano
 * in due posti — sopra la pagina e sopra lo schermo intero, come l'«Annulla»
 * della coda — e leggono lo stesso archivio, quindi dicono la stessa cosa.
 *
 * # Le risposte dei comandi e gli eventi
 *
 * Quasi ogni comando risponde con lo stato aggiornato, e il nucleo manda
 * comunque `insieme:stato` per lo stesso cambiamento. I due arrivano in un
 * ordine che non si sa: posare la risposta sopra un evento più nuovo
 * rimetterebbe a schermo il passato — una persona appena entrata che sparisce
 * di nuovo — fino al cambiamento successivo, che può non arrivare per minuti.
 * Quindi la risposta si adotta solo se nel frattempo non è arrivato nessun
 * evento (`comanda`): l'evento è sempre più fresco di lei, o uguale.
 *
 * # Niente di suo, nemmeno la calibrazione
 *
 * Nella prima stesura due cose stavano qui perché il contratto non le
 * portava: se la calibrazione era accesa, tenuto da chi l'aveva accesa, e di
 * chi era ogni riga della coda, che si sapeva solo per quella che suonava. Le
 * porta adesso il nucleo (`SessioneInsieme.calibrazione` e `.proprietari`), e
 * la copia locale se n'è andata: lo stato della calibrazione tenuto qui
 * mentiva a chi ricaricava la finestra a clic accesi, e ai membri non lo
 * diceva nessuno. Restano qui solo le cose della finestra: se il pannello è
 * aperto e in quale cornice, quale finestrella è aperta, l'ultimo avviso, gli
 * inviti mandati e il minuto in cui valgono — il nucleo elenca quelli
 * ricevuti, non quelli mandati — il livello scelto per chi sta per entrare e
 * non ancora mandato, e una copia dell'ultimo guasto della fascia, per dirlo
 * anche dentro il pannello.
 */
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";

import { creaArchivio, ugualiInSuperficie, useArchivio } from "../archivio";
import { ConfermaGesto } from "../ConfermaGesto";
import { useDati } from "../dati";
import { Dialogo } from "../Dialogo";
import { inCima, useFinestrella, useQuandoSiRitira } from "../finestrella";
import { dataOraBreve, numero } from "../formato";
import {
  LIMITE_CALIBRAZIONE_MS,
  MASSIMO_INDIRIZZI,
  MASSIMO_NOME,
  aggiungiIndirizzo,
  ancoraValidi,
  avvisoEGuasto,
  causeIrraggiungibile,
  chiaveAvviso,
  codiceDaLeggere,
  codiceLeggibile,
  daInvitare,
  fidatiCheOspitano,
  fraseNessunVicino,
  fraseProfiloRete,
  invitiAncoraAperti,
  mappaProprietari,
  nomePulito,
  ordinaFidati,
  ospitanti,
  pezziSegno,
  pezzoSessione,
  secondiRestanti,
  type Pezzo,
} from "../insieme";
import {
  eErroreIpc,
  ipc,
  type AssociazioneInsieme,
  type AvvisoInsieme,
  type DirittiInsieme,
  type FidatoInsieme,
  type Guasto,
  type InsiemeNelLettore,
  type PersonaInsieme,
  type ProprietarioInsieme,
  type StatoInsieme,
} from "../ipc";
import { t, tSe, useLingua, type Chiave } from "../lingue";
import { Trans } from "../lingue/Trans";
import { useAscolto } from "../pagine";
import { Presenza, useSpecchioPresente } from "../presenza";
import { useLettore } from "../riproduzione";
import { Annuncio } from "./Annuncio";
import { Icona } from "./Icone";
import { Interruttore } from "./Interruttore";
import { Segmentato } from "./Segmentato";
import { CursoreLatenza } from "./UscitaAudio";

// ── l'archivio ──────────────────────────────────────────────────────────────

/** Un avviso di passaggio, col suo numero: due avvisi uguali sono due avvisi. */
interface Passaggio {
  chiave: string;
  nome: string | null;
  numero: number;
}

/** Quel che la finestra sa di «Ascolta insieme». */
interface VistaInsieme {
  /**
   * L'ultimo stato del nucleo. `null` finché non ha risposto — o se non
   * risponde: allora di «Ascolta insieme» non si disegna niente, nemmeno lo
   * «spento», che sarebbe un'affermazione su una cosa che non si conosce.
   */
  stato: StatoInsieme | null;
  /** La spiegazione del firewall è da mostrare, prima di ospitare. */
  firewall: boolean;
  /**
   * «Terminare la sessione?» è da chiedere.
   *
   * Qui e non nella fascia che ha il tasto: una delle due copie della fascia
   * sta dentro gli avvisi dello schermo intero, che hanno un `transform`, e un
   * antenato trasformato fa da contenitore anche ai `position: fixed` — il
   * velo della domanda avrebbe coperto la sola striscia degli avvisi invece
   * della finestra. Le finestrelle stanno tutte in `InsiemeInApp`.
   */
  terminare: boolean;
  /**
   * Il pannello è aperto, e dove: appeso al tasto della barra, o come
   * finestrella.
   *
   * Qui e non nello stato del tasto della barra, dove stava: la barra sparisce
   * — si apre la colonna, finisce la coda, la si stringe sotto i 560 pixel — e
   * con lei moriva il pannello, o peggio restava montato e invisibile, contato
   * fra i modali. Tenuto qui, quando la barra se ne va il pannello passa alla
   * finestrella (`TastoDellaBarra`) invece di sparire sotto le dita.
   */
  pannello: "barra" | "finestra" | null;
  /** L'ultimo avviso arrivato, finché è a schermo. */
  avviso: Passaggio | null;
  /**
   * L'ultimo guasto della fascia in cima alla pagina, copiato da `App`.
   *
   * Il pannello aperto copre la pagina — la finestrella col suo velo scuro e
   * il resto `inert`, il pannello della barra col suo velo — e un guasto nato
   * da un suo tasto, «Associa» che non riesce, si disegnava **dietro**: chi
   * aveva premuto non vedeva niente succedere. Qui una copia sola, scritta da
   * `InsiemeInApp`, che il pannello disegna dentro di sé; chiuderla chiude
   * quella della fascia, perché è la stessa.
   */
  guasto: (Guasto & { chiudi: () => void }) | null;
  /** Gli inviti mandati, per pari: quando torna acceso «Invita». */
  invitati: ReadonlyMap<number, number>;
  /**
   * «Ospita una sessione» premuto mentre un associato ospita già: il suo id,
   * finché la domanda è aperta. Vedi `SceltaOspita`.
   */
  scelta: number | null;
  /**
   * Che cosa potrà fare chi sta per entrare, scelto e non ancora mandato:
   * per numero di richiesta, e per pari da invitare. Chi non c'è parte da
   * `DIRITTI_DI_PARTENZA`.
   *
   * Qui e non nel componente: la stessa richiesta si disegna due volte —
   * nella fascia sopra la pagina e nel pannello — e due scelte locali
   * avrebbero lasciato «Fai entrare» mandare quella della copia premuta,
   * diversa magari da quella appena vista nell'altra. Per lo stesso motivo
   * il pari da invitare, che compare nel pannello e nelle Impostazioni.
   */
  livelli: {
    richieste: ReadonlyMap<number, DirittiInsieme>;
    inviti: ReadonlyMap<number, DirittiInsieme>;
  };
}

/** Nessun invito mandato: una mappa vuota con un'identità sola. */
const NESSUN_INVITO: ReadonlyMap<number, number> = new Map();

/** Nessun livello scelto: una mappa vuota con un'identità sola. */
const NESSUN_LIVELLO: ReadonlyMap<number, DirittiInsieme> = new Map();

/**
 * Il livello acceso quando qualcuno chiede di entrare o lo si invita.
 *
 * `comanda`, la via di mezzo scelta dall'utente: pausa, salti e barra sì, la
 * coda resta a chi ospita — nessuno svuota la serata di un altro per sbaglio,
 * e chi vuole di più o di meno lo cambia con un clic, prima o dopo.
 */
const DIRITTI_DI_PARTENZA: DirittiInsieme = "comanda";

const ARCHIVIO = creaArchivio<VistaInsieme>({
  stato: null,
  firewall: false,
  terminare: false,
  pannello: null,
  avviso: null,
  guasto: null,
  invitati: NESSUN_INVITO,
  scelta: null,
  livelli: { richieste: NESSUN_LIVELLO, inviti: NESSUN_LIVELLO },
});

/** Quanti `insieme:stato` sono arrivati: vedi «Le risposte dei comandi». */
let eventiArrivati = 0;

/** Il numero dell'ultimo avviso. */
let avvisiArrivati = 0;

/**
 * Quanto resta a schermo una notizia, in millisecondi.
 *
 * Otto secondi, fra i sei dell'uscita spostata e i dieci dell'«Annulla» della
 * coda: si legge e basta, come la prima, ma arriva quasi sempre mentre si
 * guarda altrove — «Marco è entrato» lo si nota dal suono che cambia. I guasti
 * invece restano finché non si chiudono: «la connessione è caduta» spiega una
 * coda che è appena cambiata sotto gli occhi, e sparire da solo vorrebbe dire
 * lasciare la domanda senza risposta.
 */
const DURATA_NOTIZIA = 8_000;

/** Un guasto nel diario: solo il codice, mai il testo — vedi le regole del diario. */
function annota(dove: string, e: unknown): void {
  void ipc.diarioAnnota("insieme", `${dove}: ${eErroreIpc(e) ? e.code : "?"}`);
}

/**
 * Lo stato del nucleo, adottato.
 *
 * Una finestra del pannello aperta su un PC dove la funzione si è appena
 * spenta resta aperta: dice «spento» e offre di riaccendere.
 *
 * Con lui si rileggono le cose della finestra che dipendono dal nucleo: gli
 * inviti mandati che non valgono più (`invitiAncoraAperti`), e le domande
 * rimaste aperte su una fase che è finita — «X ospita già» quando X ha smesso
 * o questo PC non è più libero, il firewall fuori da «libera», «Terminare la
 * sessione?» quando la sessione non c'è più. Le ultime due si disegnano solo
 * nella loro fase, ma il segno restava: «Terminare?» lasciato senza risposta
 * da una sessione chiusa altrove saltava fuori da solo alla prossima volta che
 * questo PC ospitava.
 */
function adotta(stato: StatoInsieme): void {
  ARCHIVIO.scrivi((prima) => ({
    ...prima,
    stato,
    firewall: prima.firewall && stato.fase === "libera",
    terminare: prima.terminare && stato.fase === "ospita",
    invitati: invitiAncoraAperti(prima.invitati, stato, Date.now()),
    livelli: livelliAncoraAperti(prima.livelli, stato),
    scelta:
      prima.scelta !== null &&
      stato.fase === "libera" &&
      fidatiCheOspitano(stato).some((f) => f.id === prima.scelta)
        ? prima.scelta
        : null,
  }));
}

/**
 * I livelli scelti che valgono ancora: quelli delle richieste ancora in
 * attesa. Quelli dei pari da invitare restano finché si invita (`invita`):
 * il pari non sparisce dal nucleo quando si chiude il pannello.
 */
function livelliAncoraAperti(
  livelli: VistaInsieme["livelli"],
  stato: StatoInsieme,
): VistaInsieme["livelli"] {
  if (livelli.richieste.size === 0) return livelli;
  const vive = new Set(stato.richieste.map((r) => r.id));
  const restano = [...livelli.richieste].filter(([id]) => vive.has(id));
  return restano.length === livelli.richieste.size
    ? livelli
    : { ...livelli, richieste: restano.length === 0 ? NESSUN_LIVELLO : new Map(restano) };
}

/** Sceglie il livello di chi sta per entrare, prima di «Fai entrare» o «Invita». */
function scegliLivello(dove: "richieste" | "inviti", id: number, diritti: DirittiInsieme): void {
  ARCHIVIO.scrivi((prima) => ({
    ...prima,
    livelli: { ...prima.livelli, [dove]: new Map(prima.livelli[dove]).set(id, diritti) },
  }));
}

/** Il livello scelto per una richiesta o un pari, o quello di partenza. */
function useLivello(dove: "richieste" | "inviti", id: number): DirittiInsieme {
  return useArchivio(ARCHIVIO, (v) => v.livelli[dove].get(id) ?? DIRITTI_DI_PARTENZA);
}

/** Dimentica il livello scelto: è partito con la risposta o con l'invito. */
function dimenticaLivello(dove: "richieste" | "inviti", id: number): void {
  ARCHIVIO.scrivi((prima) => {
    if (!prima.livelli[dove].has(id)) return prima;
    const dopo = new Map(prima.livelli[dove]);
    dopo.delete(id);
    return { ...prima, livelli: { ...prima.livelli, [dove]: dopo } };
  });
}

/**
 * Un comando che risponde con lo stato: la risposta si adotta solo se nessun
 * evento è arrivato nel frattempo. Vedi «Le risposte dei comandi e gli
 * eventi» in testa.
 */
function comanda(azione: Promise<StatoInsieme>, onErrore: (e: unknown) => void): void {
  const prima = eventiArrivati;
  azione
    .then((stato) => {
      if (eventiArrivati === prima) adotta(stato);
    })
    .catch(onErrore);
}

/**
 * Apre il pannello come finestrella: dalla fascia, dalla colonna, dalle
 * Impostazioni.
 */
export function apriFinestra(): void {
  ARCHIVIO.scrivi((prima) => ({ ...prima, pannello: "finestra" }));
}

/** Chiude il pannello, in qualunque cornice sia. */
function chiudiPannello(): void {
  ARCHIVIO.scrivi((prima) => (prima.pannello === null ? prima : { ...prima, pannello: null }));
}

/**
 * Chiude il pannello della barra, se è aperto lì: lo chiede l'equalizzatore,
 * che si apre accanto. Vedi `Lettore.tsx`.
 */
export function chiudiPannelloDellaBarra(): void {
  ARCHIVIO.scrivi((prima) =>
    prima.pannello === "barra" ? { ...prima, pannello: null } : prima,
  );
}

/**
 * Il pannello aperto nella barra passa alla finestrella: la barra, o il suo
 * guscio, non c'è più. Vedi `TastoDellaBarra`.
 */
function passaAllaFinestra(): void {
  ARCHIVIO.scrivi((prima) =>
    prima.pannello === "barra" ? { ...prima, pannello: "finestra" } : prima,
  );
}

/**
 * «Ospita una sessione», da qualunque posto.
 *
 * Se un associato ospita già, prima la domanda: entrare nella sua o aprirne
 * un'altra (`SceltaOspita`). Due sessioni separate nella stessa stanza sono
 * quasi sempre un equivoco — la persona voleva unirsi al collega e ha premuto
 * il tasto più grande che vedeva.
 */
function ospita(onErrore: (e: unknown) => void): void {
  const stato = ARCHIVIO.leggi().stato;
  if (stato === null) return;
  const gia = fidatiCheOspitano(stato)[0];
  if (gia !== undefined) {
    ARCHIVIO.scrivi((prima) => ({ ...prima, scelta: gia.id }));
    return;
  }
  ospitaComunque(onErrore);
}

/**
 * Ospita, senza chiedere chi ospita già.
 *
 * La prima volta passa dalla spiegazione del firewall: la richiesta di Windows
 * arriva un istante dopo, e chi la vede senza sapere cosa scegliere sceglie
 * «Annulla» — che non vuol dire «non adesso» ma «mai», con una regola di blocco
 * che resta. Il dialogo sta in `InsiemeInApp`, una volta sola, e non nel posto
 * da cui si è premuto.
 */
function ospitaComunque(onErrore: (e: unknown) => void): void {
  const stato = ARCHIVIO.leggi().stato;
  if (stato === null) return;
  if (!stato.firewallSpiegato) {
    ARCHIVIO.scrivi((prima) => ({ ...prima, firewall: true }));
    return;
  }
  comanda(ipc.insiemeOspita(), onErrore);
}

/** Chiede a un associato che ospita di entrare nella sua sessione. */
function chiediDiEntrare(pari: number, onErrore: (e: unknown) => void): void {
  ipc.insiemeChiedi(pari).catch(onErrore);
}

/** Quanto vale un invito mandato: lo stesso minuto del nucleo. */
const DURATA_INVITO = 60_000;

/**
 * Invita un associato, e spegne il suo «Invita» per il minuto in cui l'invito
 * vale: premere di nuovo manderebbe un secondo invito alla stessa persona, cioè
 * due banner sul suo schermo per la stessa domanda.
 *
 * Il minuto sta nell'archivio e non nel componente: «Invita» c'è nel pannello
 * e nelle Impostazioni, e un pannello chiuso e riaperto ripartiva da zero. Alla
 * scadenza l'invito si toglie da sé, così niente resta a contare secondi per
 * un tasto che è già tornato acceso; prima di allora lo toglie il nucleo, se
 * dice che quella persona non si può più invitare (`adotta`).
 *
 * Il livello parte con l'invito e poi si dimentica: il prossimo invito alla
 * stessa persona riparte da quello di partenza, come ogni richiesta. Si
 * sceglie ogni volta.
 */
function invita(pari: number, diritti: DirittiInsieme, onErrore: (e: unknown) => void): void {
  const scade = Date.now() + DURATA_INVITO;
  const togli = (quando: number | null) =>
    ARCHIVIO.scrivi((prima) => {
      if (!prima.invitati.has(pari)) return prima;
      if (quando !== null && prima.invitati.get(pari) !== quando) return prima;
      const dopo = new Map(prima.invitati);
      dopo.delete(pari);
      return { ...prima, invitati: dopo };
    });
  ARCHIVIO.scrivi((prima) => ({ ...prima, invitati: new Map(prima.invitati).set(pari, scade) }));
  window.setTimeout(() => togli(scade), DURATA_INVITO);
  dimenticaLivello("inviti", pari);
  ipc.insiemeInvita(pari, diritti).catch((e: unknown) => {
    togli(null);
    onErrore(e);
  });
}

/**
 * Un invito mandato a cui il nucleo dice che non si risponderà più: rifiutato
 * o lasciato scadere. L'avviso porta il nome e non l'id, quindi si cerca per
 * nome — due associati omonimi riaccenderebbero tutti e due, che è il male
 * minore rispetto a un tasto spento per un minuto senza motivo.
 */
function invitoChiuso(nome: string | null): void {
  if (nome === null) return;
  ARCHIVIO.scrivi((prima) => {
    const stato = prima.stato;
    if (stato === null || prima.invitati.size === 0) return prima;
    const suoi = new Set(stato.fidati.filter((f) => f.nome === nome).map((f) => f.id));
    const restano = [...prima.invitati].filter(([pari]) => !suoi.has(pari));
    return restano.length === prima.invitati.size
      ? prima
      : { ...prima, invitati: new Map(restano) };
  });
}

/**
 * Dopo la spiegazione: si segna vista, e si ospita.
 *
 * Si ospita anche se il segno non si è scritto — la persona ha letto e ha
 * detto sì, e farle rileggere tutto alla prossima apertura è il prezzo minore.
 */
function ospitaDopoIlFirewall(onErrore: (e: unknown) => void): void {
  ipc
    .insiemeFirewallVisto()
    .catch((e: unknown) => annota("firewall", e))
    .then(() => comanda(ipc.insiemeOspita(), onErrore));
}

/** Chi ospita termina per tutti; chi segue (o sta entrando) esce. */
function esci(ospita: boolean, onErrore: (e: unknown) => void): void {
  comanda(ospita ? ipc.insiemeTermina() : ipc.insiemeEsci(), onErrore);
}

/**
 * Accende o spegne i clic della calibrazione.
 *
 * Senza toccare niente qui: il tasto si accende quando il nucleo lo dice, con
 * `SessioneInsieme.calibrazione` nel prossimo stato. Accenderlo al clic
 * vorrebbe dire un tasto acceso su clic che magari non sono partiti.
 */
function calibra(attiva: boolean, onErrore: (e: unknown) => void): void {
  ipc.insiemeCalibra(attiva).catch(onErrore);
}

/**
 * Toglie un invito o una richiesta a cui si è appena risposto.
 *
 * Subito, senza aspettare il nucleo: il secondo clic di chi ha premuto due
 * volte non deve trovare il tasto ancora lì. Il prossimo stato dirà il vero.
 */
function risposto(quale: "inviti" | "richieste", id: number): void {
  ARCHIVIO.scrivi((prima) => {
    const stato = prima.stato;
    if (stato === null) return prima;
    return {
      ...prima,
      stato:
        quale === "inviti"
          ? { ...stato, inviti: stato.inviti.filter((v) => v.id !== id) }
          : { ...stato, richieste: stato.richieste.filter((v) => v.id !== id) },
    };
  });
}

/** Lo stato del nucleo, per chi disegna. */
function useStatoInsieme(): StatoInsieme | null {
  return useArchivio(ARCHIVIO, (v) => v.stato);
}

/** Nessun proprietario: per chi non è in una sessione. Un'identità sola. */
const NESSUNO: readonly ProprietarioInsieme[] = [];

/**
 * Di chi sono le voci della coda che non sono di questo PC, per maniglia.
 *
 * Per `Coda.tsx`, che disegna «di Marco» su ogni riga. L'elenco cambia solo
 * con `insieme:stato`, e la mappa si rifà solo allora: una coda di mille righe
 * che si ridisegna a ogni passo del volume non deve ricostruirla.
 */
export function useProprietari(): ReadonlyMap<number, string> {
  const elenco = useArchivio(ARCHIVIO, (v) => v.stato?.sessione?.proprietari ?? NESSUNO);
  return useMemo(() => mappaProprietari(elenco), [elenco]);
}

/** Il campo `insieme` dello stato della riproduzione, senza svegliarsi per il resto. */
function useNelLettore(): InsiemeNelLettore | null {
  return useLettore(
    (s) => s.insieme,
    (a, b) => a === b || (a !== null && b !== null && ugualiInSuperficie(a, b)),
  );
}

/**
 * L'ora di parete, rinfrescata ogni secondo finché serve.
 *
 * Solo finché c'è un conto alla rovescia a schermo: un intervallo acceso per
 * tutta la vita della finestra sveglierebbe React ogni secondo per non
 * disegnare niente — la stessa disciplina del ciclo dei fotogrammi in
 * `riproduzione.ts`.
 */
function useAdesso(acceso: boolean): number {
  const [adesso, setAdesso] = useState(() => Date.now());
  useEffect(() => {
    if (!acceso) return;
    setAdesso(Date.now());
    const giro = window.setInterval(() => setAdesso(Date.now()), 1000);
    return () => window.clearInterval(giro);
  }, [acceso]);
  return adesso;
}

/** Un pezzo di frase, tradotto. */
function frase(pezzo: Pezzo): string {
  return t(pezzo.chiave, pezzo.valori);
}

/** Il nome di un PC, o «PC senza nome» quando non l'ha ancora detto. */
function nomeDi(nome: string | null): string {
  return nome ?? t("together.nearby.unnamed");
}

// ── il montaggio unico ─────────────────────────────────────────────────────

/**
 * Gli ascolti e le finestrelle di «Ascolta insieme», una volta per finestra.
 *
 * Si monta in `App` fuori dalle due copie della pagina: due ascoltatori dello
 * stesso evento conterebbero ogni stato due volte, e due finestre del codice
 * una sopra l'altra chiederebbero due risposte alla stessa domanda.
 */
export function InsiemeInApp({
  onErrore,
  guasto,
  onChiudiGuasto,
}: {
  onErrore: (e: unknown) => void;
  /** Il guasto della fascia in cima alla pagina: vedi `VistaInsieme.guasto`. */
  guasto: Guasto | null;
  /** Chiude il guasto della fascia: lo stesso gesto della sua ×. */
  onChiudiGuasto: () => void;
}) {
  const vista = useArchivio(ARCHIVIO);

  // L'ascolto prima, la fotografia dopo: chiesta prima che l'ascolto fosse
  // aperto, un cambiamento arrivato nel mezzo andava perso, e la fotografia —
  // già scattata — non lo conteneva. Vedi `poi` in `useAscolto`. Un guasto
  // qui non si mostra: è una funzione spenta di serie, e una fascia rossa a
  // ogni avvio per una cosa che quasi nessuno usa sarebbe rumore. Va nel
  // diario, e la scheda delle Impostazioni dice che non risponde. Un evento
  // arrivato prima della risposta vince su di lei.
  const vivo = useRef(true);
  useEffect(() => {
    vivo.current = true;
    return () => {
      vivo.current = false;
    };
  }, []);
  const fotografia = () => {
    const prima = eventiArrivati;
    ipc
      .insiemeStato()
      .then((stato) => {
        if (vivo.current && eventiArrivati === prima) adotta(stato);
      })
      .catch((e: unknown) => annota("stato", e));
  };

  useAscolto<StatoInsieme>(
    "insieme:stato",
    (stato) => {
      eventiArrivati += 1;
      adotta(stato);
    },
    true,
    fotografia,
  );

  useAscolto<AvvisoInsieme>("insieme:avviso", (avviso) => {
    const chiave = chiaveAvviso(avviso.chiave);
    if (chiave === null) {
      // Nel diario senza la chiave: quel che non si riconosce non si
      // ricopia, nemmeno lì.
      void ipc.diarioAnnota("insieme", "avviso scartato");
      return;
    }
    if (
      chiave === "together.notice.inviteDeclined" ||
      chiave === "together.notice.inviteUnanswered"
    ) {
      invitoChiuso(avviso.nome);
    }
    avvisiArrivati += 1;
    const numero = avvisiArrivati;
    ARCHIVIO.scrivi((prima) => ({
      ...prima,
      avviso: { chiave, nome: avviso.nome, numero },
    }));
  });

  // Il guasto della fascia, copiato per il pannello. Per le due frasi e non per
  // l'oggetto: `App` lo ricostruisce a ogni disegno — venti volte al secondo
  // mentre suona — e copiarlo ogni volta sveglierebbe il pannello per niente.
  // La chiusura in un riferimento, per la ragione di `useFinestrella`.
  const chiudiGuasto = useRef(onChiudiGuasto);
  chiudiGuasto.current = onChiudiGuasto;
  const testoGuasto = guasto?.testo ?? null;
  const dettaglioGuasto = guasto?.dettaglio ?? null;
  useEffect(() => {
    ARCHIVIO.scrivi((prima) => {
      if (testoGuasto === null) return prima.guasto === null ? prima : { ...prima, guasto: null };
      return {
        ...prima,
        guasto: {
          testo: testoGuasto,
          dettaglio: dettaglioGuasto,
          chiudi: () => chiudiGuasto.current(),
        },
      };
    });
  }, [testoGuasto, dettaglioGuasto]);

  const chiudiFirewall = () => ARCHIVIO.scrivi((prima) => ({ ...prima, firewall: false }));
  const chiudiTerminare = () => ARCHIVIO.scrivi((prima) => ({ ...prima, terminare: false }));
  const chiudiScelta = () => ARCHIVIO.scrivi((prima) => ({ ...prima, scelta: null }));
  const stato = vista.stato;
  const daScegliere =
    vista.scelta === null || stato === null
      ? undefined
      : fidatiCheOspitano(stato).find((f) => f.id === vista.scelta);

  return (
    <>
      {/* Nell'ordine in cui si possono aprire: la finestra del pannello per
          prima, perché il codice, la scelta e il firewall nascono **da** lei —
          premendo «Associa» o «Ospita» — e devono starle sopra. */}
      {vista.pannello === "finestra" && stato !== null && <FinestraInsieme onErrore={onErrore} />}
      {stato?.associazione != null && (
        <DialogoCodice
          // Un'associazione nuova è una domanda nuova: il fuoco riparte dal
          // primo tasto invece di restare dov'era la risposta di prima.
          key={stato.associazione.id}
          associazione={stato.associazione}
          onErrore={onErrore}
        />
      )}
      {daScegliere !== undefined && (
        <SceltaOspita fidato={daScegliere} onChiudi={chiudiScelta} onErrore={onErrore} />
      )}
      {vista.firewall && (
        <ConfermaGesto
          domanda={{
            titolo: t("together.firewall.title"),
            descrizione: t("together.firewall.body"),
            conferma: t("together.firewall.confirm"),
            fai: () => ospitaDopoIlFirewall(onErrore),
          }}
          onChiudi={chiudiFirewall}
        />
      )}
      {vista.terminare && stato?.fase === "ospita" && (
        <ConfermaGesto
          domanda={{
            titolo: t("together.end.confirm.title"),
            descrizione: t("together.end.confirm.body"),
            conferma: t("together.end.confirm"),
            fai: () => esci(true, onErrore),
          }}
          onChiudi={chiudiTerminare}
        />
      )}
    </>
  );
}

// ── il codice ───────────────────────────────────────────────────────────────

/**
 * «Il codice è lo stesso sull'altro PC?»
 *
 * Compare sui **due** PC, anche su quello che non ha premuto niente: è il
 * nucleo a dire che un'associazione è in corso, e chi ospita può non stare
 * guardando il pannello.
 *
 * Il primo tasto, e quello che prende il fuoco, è «No»: un Invio dato di
 * fretta deve rifiutare, non fidarsi. Lo stesso nodo fa da «Annulla» mentre il
 * codice si calcola e dopo aver detto sì — cambia la scritta, non l'elemento,
 * così il fuoco non cade sul `<body>` quando il codice arriva. Escape vale no;
 * il velo non chiude, perché un clic accanto alla finestrella non è una
 * risposta a una domanda di sicurezza.
 */
function DialogoCodice({
  associazione,
  onErrore,
}: {
  associazione: AssociazioneInsieme;
  onErrore: (e: unknown) => void;
}) {
  const rispondi = (uguale: boolean) => {
    ipc.insiemeCodice(associazione.id, uguale).catch(onErrore);
  };
  const codice = associazione.codice;
  const daDecidere = codice !== null && !associazione.confermato;

  return (
    <Dialogo
      titolo={t("together.code.title", { nome: associazione.nome })}
      descrizione={
        codice === null
          ? t("together.code.computing")
          : associazione.confermato
            ? t("together.code.waiting")
            : t("together.code.question")
      }
      classe="finestrella stretta glass-modal"
      onChiudi={() => rispondi(false)}
      veloChiude={false}
    >
      {/* Il codice arriva dopo l'apertura, quindi lo dice una regione viva e
          non la descrizione, che si legge una volta sola. Cifra per cifra:
          vedi `codiceDaLeggere`. */}
      <Annuncio
        testo={codice === null ? null : t("together.code.aria", { cifre: codiceDaLeggere(codice) })}
      />
      {codice !== null && (
        <div className="codice-insieme" aria-hidden="true">
          {codiceLeggibile(codice)}
        </div>
      )}
      {daDecidere && <p>{t("together.code.hint")}</p>}
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={() => rispondi(false)}>
          {daDecidere ? t("together.code.different") : t("common.cancel")}
        </button>
        {daDecidere && (
          <button type="button" className="bottone primario" onClick={() => rispondi(true)}>
            {t("together.code.same")}
          </button>
        )}
      </div>
    </Dialogo>
  );
}

/**
 * «Marco ospita già una sessione»: entrare nella sua, o ospitarne un'altra.
 *
 * Nella forma di `ConfermaGesto`, con un tasto in più: qui le risposte sono
 * tre e nessuna toglie niente, quindi il primario va al gesto che quasi sempre
 * si cercava — entrare — e «Ospita comunque» resta un tasto normale, che passa
 * dalla spiegazione del firewall come ogni «Ospita». Il fuoco aprendo va su
 * «Annulla», il primo: un Invio di fretta non deve aprire porte né mandare
 * richieste.
 */
function SceltaOspita({
  fidato,
  onChiudi,
  onErrore,
}: {
  fidato: FidatoInsieme;
  onChiudi: () => void;
  onErrore: (e: unknown) => void;
}) {
  return (
    <Dialogo
      titolo={t("together.host.already.title", { nome: fidato.nome })}
      descrizione={t("together.host.already.body")}
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
    >
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="bottone"
          onClick={() => {
            onChiudi();
            ospitaComunque(onErrore);
          }}
        >
          {t("together.host.already.anyway")}
        </button>
        <button
          type="button"
          className="bottone primario"
          onClick={() => {
            onChiudi();
            chiediDiEntrare(fidato.id, onErrore);
          }}
        >
          {t("together.join.of", { nome: fidato.nome })}
        </button>
      </div>
    </Dialogo>
  );
}

// ── la fascia, gli inviti, gli avvisi ──────────────────────────────────────

/**
 * Quel che «Ascolta insieme» ha da dire sopra la pagina: l'ultimo avviso e la
 * sessione in corso.
 *
 * Montato in due posti come l'«Annulla» della coda: sopra la pagina, e sopra
 * lo schermo intero, che copre la pagina e la rende `inert`. Gli inviti stanno
 * a parte, in `InvitiInsieme`: vedi lì perché.
 */
export function AvvisiInsieme({ onErrore }: { onErrore: (e: unknown) => void }) {
  return (
    <>
      <AvvisoDiPassaggio onErrore={onErrore} />
      <FasciaInsieme onErrore={onErrore} />
    </>
  );
}

/**
 * Gli inviti e le richieste che aspettano, sopra la pagina.
 *
 * Separati dal resto perché sono gli unici che restano in vista a elenco
 * scorso, e stanno nello stesso contenitore aderente dell'«Annulla» della coda
 * (`.aderenti-pagina` in `App.tsx`): ognuno aderente per conto suo, due inviti
 * e un «Annulla» si fermavano tutti nello stesso punto, uno sopra l'altro, e
 * si leggeva solo l'ultimo.
 */
export function InvitiInsieme({ onErrore }: { onErrore: (e: unknown) => void }) {
  return <InAttesa dove="fascia" onErrore={onErrore} />;
}

/**
 * La sessione in corso, in cima alla pagina, con il tasto per uscirne.
 *
 * # Perché qui e non solo nella barra
 *
 * Perché la barra non c'è sempre. Si toglie quando non c'è un brano — e chi è
 * appena entrato in una sessione dove l'host non ha ancora fatto partire
 * niente è esattamente in quel caso — e non c'è quando la terza colonna è
 * aperta. Un «Esci» che sparisce proprio quando non si sente niente è un
 * «Esci» che non si trova. Qui sta finché la sessione dura.
 *
 * Dice anche, a chi segue, perché i tasti del lettore sono spenti: un tasto
 * spento non mostra il suo suggerimento e non prende il fuoco, quindi la
 * spiegazione scritta sul tasto non la legge nessuno.
 */
function FasciaInsieme({ onErrore }: { onErrore: (e: unknown) => void }) {
  const stato = useStatoInsieme();
  const insieme = useNelLettore();
  if (stato === null) return null;
  const { fase, sessione } = stato;
  if (fase !== "entrando" && fase !== "ospita" && fase !== "membro" && fase !== "uscendo") {
    return null;
  }

  const ospito = fase === "ospita";
  const testo =
    fase === "entrando"
      ? sessione === null
        ? t("together.entering")
        : t("together.entering.of", { nome: sessione.host })
      : fase === "uscendo" || sessione === null
        ? t("together.leaving")
        : frase(
            pezzoSessione({
              ospita: sessione.mia,
              host: sessione.host,
              persone: sessione.persone.length,
            }),
          );
  // La seconda riga, una cosa sola e la più urgente: i clic della
  // calibrazione — che spiegano un suono che altrimenti sembra un guasto — poi,
  // per chi segue, i comandi che chi ospita non gli ha dato e l'aggancio.
  const sotto =
    sessione?.calibrazione === true
      ? t("together.calibrate.running")
      : fase !== "membro"
        ? null
        : insieme?.diritti === "ascolta"
          ? t("together.rights.line.listen")
          : insieme?.diritti === "comanda"
            ? t("together.rights.line.control")
            : stato.sincronia !== null && !stato.sincronia.agganciato
              ? t("together.sync.locking")
              : null;

  const termina = () => {
    // Una conferma solo se c'è qualcuno da lasciare a metà brano: chiudere una
    // sessione dove si è da soli non toglie niente a nessuno. La domanda la
    // disegna `InsiemeInApp`: vedi `VistaInsieme.terminare`.
    if ((sessione?.persone.length ?? 0) > 1) {
      ARCHIVIO.scrivi((prima) => ({ ...prima, terminare: true }));
    } else {
      esci(true, onErrore);
    }
  };

  return (
    <div
      className="notizia toast-card fascia-insieme"
      role="group"
      aria-label={t("together.title")}
    >
      <Icona nome="i-insieme" dim={16} />
      <span>
        <strong>{testo}</strong>
        {sotto !== null && <small className="sotto-insieme">{sotto}</small>}
      </span>
      {fase !== "uscendo" && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={t("together.open")}
          onClick={apriFinestra}
        >
          {t("together.open.short")}
        </button>
      )}
      {fase === "entrando" && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={() => esci(false, onErrore)}
        >
          {t("common.cancel")}
        </button>
      )}
      {(fase === "ospita" || fase === "membro") && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={ospito ? t("together.end") : t("together.leave")}
          onClick={ospito ? termina : () => esci(false, onErrore)}
        >
          {ospito ? t("together.end.short") : t("together.leave.short")}
        </button>
      )}
    </div>
  );
}

/**
 * Gli inviti ricevuti e, per chi ospita, le richieste d'ingresso: con il conto
 * alla rovescia e i due tasti.
 *
 * In due vesti: `fascia` sopra la pagina, come l'«Annulla» della coda —
 * aderente in cima anche a elenco scorso, perché un invito che scade in un
 * minuto non può stare fuori vista — e `pannello` dentro il pannello, che
 * quando è aperto copre la pagina: senza questa seconda veste, un invito
 * arrivato col pannello aperto si vedrebbe e non si potrebbe accettare. È la
 * stessa ragione per cui nel pannello ci sono anche l'ultimo avviso e
 * l'ultimo guasto (`NotizieNelPannello`).
 */
function InAttesa({
  dove,
  onErrore,
}: {
  dove: "fascia" | "pannello";
  onErrore: (e: unknown) => void;
}) {
  const stato = useStatoInsieme();
  const inviti = stato?.inviti ?? [];
  const richieste = stato?.richieste ?? [];
  const adesso = useAdesso(inviti.length + richieste.length > 0);
  const vivi = [
    ...ancoraValidi(inviti, adesso).map((v) => ({ ...v, tipo: "invito" as const })),
    ...ancoraValidi(richieste, adesso).map((v) => ({ ...v, tipo: "richiesta" as const })),
  ];

  // Si annuncia l'arrivo, non il conto: una frase che cambia ogni secondo
  // sarebbe un lettore di schermo che non sta mai zitto. Vedi `Annuncio.tsx`.
  const ultimo = vivi.at(-1);
  const annuncio =
    dove === "fascia" && ultimo !== undefined
      ? ultimo.tipo === "invito"
        ? t("together.invite.from", { nome: ultimo.da })
        : t("together.request.from", { nome: ultimo.da })
      : null;

  const righe = vivi.map((voce) => {
    const secondi = secondiRestanti(voce.scadeMs, adesso);
    const invito = voce.tipo === "invito";
    // Il livello si legge al clic e non al disegno: è nell'archivio, e la
    // copia nell'altra veste può averlo appena cambiato.
    const livello = () => ARCHIVIO.leggi().livelli.richieste.get(voce.id) ?? DIRITTI_DI_PARTENZA;
    const accetta = () => {
      risposto(invito ? "inviti" : "richieste", voce.id);
      (invito
        ? ipc.insiemeRispondi(voce.id, true)
        : ipc.insiemeAmmetti(voce.id, true, livello())
      ).catch(onErrore);
    };
    const rifiuta = () => {
      risposto(invito ? "inviti" : "richieste", voce.id);
      (invito
        ? ipc.insiemeRispondi(voce.id, false)
        : ipc.insiemeAmmetti(voce.id, false, livello())
      ).catch(onErrore);
    };
    const contenuto = (
      <>
        <span className="chi-insieme">
          {invito
            ? t("together.invite.from", { nome: voce.da })
            : t("together.request.from", { nome: voce.da })}
        </span>
        <span className="conto-insieme" title={t("together.countdown.aria", { n: secondi })}>
          {t("together.countdown", { s: String(secondi) })}
        </span>
        {!invito && <LivelloDaScegliere dove="richieste" id={voce.id} nome={voce.da} />}
        <button
          type="button"
          className="bottone minuto primario"
          aria-label={
            invito
              ? t("together.accept.aria", { nome: voce.da })
              : t("together.admit.aria", { nome: voce.da })
          }
          onClick={accetta}
        >
          {invito ? t("together.accept") : t("together.admit")}
        </button>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={
            invito
              ? t("together.decline.aria", { nome: voce.da })
              : t("together.refuse.aria", { nome: voce.da })
          }
          onClick={rifiuta}
        >
          {t("together.decline")}
        </button>
      </>
    );
    return dove === "fascia" ? (
      <div
        key={`${voce.tipo}-${voce.id}`}
        className={
          invito
            ? "notizia toast-card attesa-insieme"
            : "notizia toast-card attesa-insieme con-diritti"
        }
      >
        <Icona nome="i-insieme" dim={16} />
        {contenuto}
      </div>
    ) : (
      <li
        key={`${voce.tipo}-${voce.id}`}
        className={invito ? "riga-insieme" : "riga-insieme con-diritti"}
      >
        {contenuto}
      </li>
    );
  });

  // Sopra la pagina la regione c'è sempre, anche senza inviti: deve esistere
  // **prima** che arrivi la frase da dire. Vedi `Annuncio.tsx`.
  if (dove === "fascia") {
    return (
      <>
        <Annuncio testo={annuncio} />
        {righe}
      </>
    );
  }
  if (vivi.length === 0) return null;
  return (
    <section className="gruppo-insieme">
      <h3>{t("together.pending.title")}</h3>
      <ul className="elenco-insieme">{righe}</ul>
    </section>
  );
}

/**
 * L'ultimo avviso del nucleo: «Marco è entrato», «la connessione è caduta».
 *
 * La chiave arriva dal nucleo già controllata (`chiaveAvviso`), e si traduce
 * qui: una chiave che questa versione non conosce — un nucleo più nuovo della
 * finestra, che non succede, o un refuso, che succede — dice una frase
 * generica invece di mostrare sé stessa. Il nome è l'unico testo che arriva da
 * fuori, ed entra solo come valore.
 *
 * «Marco è associato», quando Marco ospita, porta anche «Entra nella
 * sessione»: è il momento in cui chi ha appena confrontato il codice cerca il
 * gesto successivo, e fino a qui doveva sapere che stava in un'altra riga,
 * con un altro nome — «Chiedi di unirti».
 *
 * Montato anche dentro il pannello (`NotizieNelPannello`), con `parla` spento
 * quando la copia sopra la pagina parla già: vedi lì.
 */
function AvvisoDiPassaggio({
  onErrore,
  parla = true,
}: {
  onErrore: (e: unknown) => void;
  parla?: boolean;
}) {
  const avviso = useArchivio(ARCHIVIO, (v) => v.avviso);
  const stato = useStatoInsieme();
  const guasto = avviso !== null && avvisoEGuasto(avviso.chiave);

  useEffect(() => {
    if (avviso === null || guasto) return;
    const numero = avviso.numero;
    const conto = window.setTimeout(() => chiudiAvviso(numero), DURATA_NOTIZIA);
    return () => window.clearTimeout(conto);
  }, [avviso, guasto]);

  const testo =
    avviso === null
      ? null
      : tSe(avviso.chiave, t("together.notice.generic"), { nome: nomeDi(avviso.nome) });
  const daRaggiungere =
    avviso?.chiave === "together.notice.paired" && stato?.fase === "libera"
      ? fidatiCheOspitano(stato).find((f) => f.nome === avviso.nome)
      : undefined;

  return (
    <>
      {/* Il guasto parla da sé, `alert` sul riquadro come la fascia d'errore;
          la notizia la dice la regione sempre montata. */}
      {parla && <Annuncio testo={guasto ? null : testo} />}
      {avviso !== null && (
        <Presenza parte="toast-card">
          <div
            className={guasto ? "errore toast-card" : "notizia toast-card"}
            role={guasto && parla ? "alert" : undefined}
          >
            <Icona nome={guasto ? "i-alert" : "i-insieme"} dim={16} />
            <span>{testo}</span>
            {daRaggiungere !== undefined && (
              <button
                type="button"
                className="bottone minuto primario"
                aria-label={t("together.join.of", { nome: daRaggiungere.nome })}
                onClick={() => {
                  chiudiAvviso(avviso.numero);
                  chiediDiEntrare(daRaggiungere.id, onErrore);
                }}
              >
                {t("together.join")}
              </button>
            )}
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("common.close")}
              onClick={() => chiudiAvviso(avviso.numero)}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </div>
        </Presenza>
      )}
    </>
  );
}

/**
 * L'ultimo guasto della fascia, dentro il pannello. Vedi `VistaInsieme.guasto`.
 *
 * La stessa forma della fascia d'errore (`Fascia.tsx`) senza «Riprova»: quel
 * tasto riguarda il brano che non si apre, e torna nella fascia appena il
 * pannello si chiude.
 */
function GuastoNelPannello({ parla }: { parla: boolean }) {
  const guasto = useArchivio(ARCHIVIO, (v) => v.guasto);
  return (
    <>
      {parla && <Annuncio testo={guasto?.testo ?? null} />}
      {guasto !== null && (
        <Presenza parte="toast-card">
          <div className="errore toast-card">
            <Icona nome="i-alert" dim={16} />
            <span>
              {guasto.testo}
              {guasto.dettaglio !== null && (
                <span className="dettaglio-errore">{guasto.dettaglio}</span>
              )}
            </span>
            <button
              type="button"
              className="tasto icon-btn"
              aria-label={t("toast.dismiss")}
              onClick={guasto.chiudi}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </div>
        </Presenza>
      )}
    </>
  );
}

/**
 * L'ultimo guasto e l'ultimo avviso, in cima al pannello.
 *
 * Il pannello copre la pagina, e le due fasce che li dicono stanno sotto: un
 * guasto nato da un tasto del pannello si disegnava dietro il velo, e chi
 * aveva premuto non vedeva succedere niente. Qui le stesse due cose, dagli
 * stessi archivi: chiuderle qui le chiude anche là.
 *
 * `parla` dice se tocca a questa copia annunciarle. Nella finestrella sì: la
 * pagina dietro è `inert`, e per chi ascolta lo schermo le fasce là sotto non
 * esistono. Nel pannello della barra no: la pagina resta viva, le fasce dicono
 * già la loro, e due copie la direbbero due volte.
 */
function NotizieNelPannello({
  parla,
  onErrore,
}: {
  parla: boolean;
  onErrore: (e: unknown) => void;
}) {
  return (
    <>
      <GuastoNelPannello parla={parla} />
      <AvvisoDiPassaggio parla={parla} onErrore={onErrore} />
    </>
  );
}

/** Chiude l'avviso `numero`, se è ancora quello a schermo. */
function chiudiAvviso(numero: number): void {
  ARCHIVIO.scrivi((prima) =>
    prima.avviso?.numero === numero ? { ...prima, avviso: null } : prima,
  );
}

// ── i tasti ─────────────────────────────────────────────────────────────────

/**
 * Il tasto «Ascolta insieme» nella barra del lettore, col suo pannello.
 *
 * Accanto all'equalizzatore e dentro lo stesso guscio `.con-pannello`, che il
 * foglio ritira sotto i 560 pixel di barra. Vale anche qui la regola con cui
 * si ritira l'equalizzatore: la cosa resta raggiungibile — dalla fascia della
 * sessione, dal tasto nella colonna e dalla scheda delle Impostazioni.
 *
 * C'è anche a funzione spenta, ed è voluto: è il posto dove la si scopre, e il
 * pannello dice cos'è e la accende. Non c'è quando il nucleo non ha risposto,
 * né nell'anteprima dello Studio, che disegna un lettore finto.
 *
 * Se è aperto lo dice l'archivio (`VistaInsieme.pannello`), non il tasto: vedi
 * lì. `onApri` è per chi gli sta accanto — l'equalizzatore, che si chiude:
 * i due pannelli si aprono nello stesso punto, e uno sopra l'altro si
 * coprivano a metà.
 */
export function BottoneInsieme({
  onErrore,
  onApri,
}: {
  onErrore: (e: unknown) => void;
  onApri: () => void;
}) {
  const { anteprima } = useDati();
  const stato = useStatoInsieme();
  if (anteprima || stato === null) return null;
  return (
    <TastoDellaBarra
      inSessione={stato.sessione !== null}
      onErrore={onErrore}
      onApri={onApri}
    />
  );
}

/**
 * Il tasto della barra e il suo pannello, quando ci sono.
 *
 * In un componente suo perché i ganci qui sotto valgono solo quando il tasto
 * c'è, cioè dopo le due uscite di `BottoneInsieme`: il loro smontaggio è
 * proprio il segno che il tasto se n'è andato.
 *
 * Quando il guscio sparisce — la barra si stringe sotto i 560 pixel e il
 * foglio lo ritira, o la barra stessa se ne va — un pannello aperto qui passa
 * alla finestrella (`FinestraInsieme`): chi lo stava usando lo ritrova al
 * centro invece di perderlo, e non resta un pannello invisibile contato fra i
 * modali.
 */
function TastoDellaBarra({
  inSessione,
  onErrore,
  onApri,
}: {
  inSessione: boolean;
  onErrore: (e: unknown) => void;
  onApri: () => void;
}) {
  // Dalla presenza, per l'entrata e l'uscita animate come prima, quando lo
  // stato stava qui.
  const aperto = useSpecchioPresente(ARCHIVIO, (v) => v.pannello === "barra");
  const guscio = useQuandoSiRitira(passaAllaFinestra);
  // Smontato — la barra se n'è andata — vale lo stesso.
  useEffect(() => passaAllaFinestra, []);

  const alterna = () => {
    if (aperto) {
      chiudiPannello();
      return;
    }
    onApri();
    ARCHIVIO.scrivi((prima) => ({ ...prima, pannello: "barra" }));
  };

  return (
    <div ref={guscio} className="con-pannello">
      <button
        type="button"
        className="tasto icon-btn tasto-insieme"
        aria-pressed={aperto}
        aria-expanded={aperto}
        aria-label={t("together.title")}
        title={inSessione ? t("together.inSession") : t("together.title")}
        data-acceso={inSessione || undefined}
        onClick={alterna}
      >
        <Icona nome="i-insieme" dim={17} />
      </button>
      {aperto && <PannelloInsieme onChiudi={chiudiPannello} onErrore={onErrore} />}
    </div>
  );
}

/**
 * Il tasto nella testata della terza colonna: apre il pannello come
 * finestrella.
 *
 * Esiste perché con la colonna aperta la barra non c'è, e con lei il tasto
 * qui sopra. Anche a funzione spenta, come quello della barra: lo era solo a
 * funzione accesa, perché un tasto per una funzione spenta di serie sarebbe
 * stato sotto gli occhi di tutti per niente, ma con la colonna aperta questo
 * era l'**unico** tasto, e chi accendeva la funzione e cercava dove fossero i
 * PC che ospitano non lo trovava. Aperto a funzione spenta, il pannello dice
 * cos'è e la accende.
 */
export function TastoInsieme() {
  const { anteprima } = useDati();
  const stato = useStatoInsieme();
  if (anteprima || stato === null) return null;
  const inSessione = stato.sessione !== null;
  return (
    <button
      type="button"
      className="tasto icon-btn tasto-insieme"
      aria-label={t("together.open")}
      title={inSessione ? t("together.inSession") : t("together.title")}
      data-acceso={inSessione || undefined}
      onClick={apriFinestra}
    >
      <Icona nome="i-insieme" dim={15} />
    </button>
  );
}

/**
 * Il segno della sessione accanto a quel che suona: «aggancio… · In ascolto
 * con Marco · 3 persone · in streaming da Marco». L'urgente in testa, perché
 * è la parte che i puntini mangiavano: vedi `pezziSegno`.
 *
 * Prende `insieme` per prop e non dall'archivio del lettore, come il resto
 * della barra e della colonna: lo Studio le disegna con uno stato finto, e il
 * segno deve stare a quello. Nella barra è una riga sotto il cursore, con il
 * vestito della riga dei dati del file; nella colonna sta sotto artista e
 * album, col vestito di quella riga.
 */
export function SegnoInsieme({
  insieme,
  dove,
}: {
  insieme: InsiemeNelLettore | null;
  dove: "barra" | "colonna";
}) {
  useLingua();
  if (insieme === null) return null;
  const testo = pezziSegno(insieme).map(frase).join(" · ");
  return (
    <div
      className={
        dove === "barra"
          ? "formato lettore-formato segno-insieme"
          : "meta np-meta segno-insieme"
      }
      title={testo}
    >
      {testo}
    </div>
  );
}

// ── il pannello ─────────────────────────────────────────────────────────────

/**
 * Il pannello appeso al tasto della barra, nello stile di quello
 * dell'equalizzatore (`PannelloEq`), velo compreso.
 *
 * Una differenza sola nell'Escape: quello dell'equalizzatore lo prende sempre,
 * in cattura su `window`, per non lasciarlo alle scorciatoie globali; qui lo
 * prende **solo se il fuoco è nel pannello**. Da qui si aprono il codice e il
 * firewall, che sono finestrelle sopra di lui: un Escape dato a loro chiudeva
 * anche il pannello sotto, e chi rispondeva «No» al codice si ritrovava senza
 * il pannello da cui era partito. Finché il pannello è aperto e niente gli sta
 * sopra, il fuoco è suo — la trappola di `useFinestrella` ce lo tiene.
 *
 * Suo è anche un fuoco caduto sul `<body>`, se niente gli sta sopra
 * (`inCima`): succede quando il tasto premuto sparisce col gesto che ha
 * fatto, e lì un Escape ignorato voleva dire un pannello che non si chiudeva
 * più da tastiera. `useFinestrella` ora rimette il fuoco sulla radice, e
 * questa è la seconda cintura.
 */
function PannelloInsieme({
  onChiudi,
  onErrore,
}: {
  onChiudi: () => void;
  onErrore: (e: unknown) => void;
}) {
  const { anteprima } = useDati();
  const radice = useFinestrella<HTMLDivElement>(onChiudi);
  const idTitolo = useId();
  useEffect(() => {
    if (anteprima) return;
    const suTasto = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const dentro = radice.current;
      if (dentro === null) return;
      const fuoco = document.activeElement;
      const caduto = (fuoco === null || fuoco === document.body) && inCima(dentro);
      if (!caduto && !dentro.contains(fuoco)) return;
      e.preventDefault();
      e.stopPropagation();
      onChiudi();
    };
    window.addEventListener("keydown", suTasto, { capture: true });
    return () => window.removeEventListener("keydown", suTasto, { capture: true });
  }, [onChiudi, anteprima, radice]);

  return (
    <>
      <div className="velo" onClick={onChiudi} />
      <Presenza parte="eq-panel">
        <div
          ref={radice}
          className="pannello-eq pannello-insieme eq-panel"
          role="dialog"
          aria-modal="true"
          aria-labelledby={idTitolo}
        >
          <h2 id={idTitolo} className="titolo-insieme">
            {t("together.title")}
          </h2>
          <ContenutoInsieme parla={false} onErrore={onErrore} />
        </div>
      </Presenza>
    </>
  );
}

/**
 * Lo stesso pannello come finestrella, per chi non ha la barra sotto mano:
 * dalla fascia della sessione, dalla terza colonna, dalle Impostazioni, e
 * quando la barra se ne va col pannello aperto.
 */
function FinestraInsieme({ onErrore }: { onErrore: (e: unknown) => void }) {
  return (
    <Dialogo
      titolo={t("together.title")}
      classe="finestrella stretta glass-modal"
      onChiudi={chiudiPannello}
    >
      <ContenutoInsieme parla onErrore={onErrore} />
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={chiudiPannello}>
          {t("common.close")}
        </button>
      </div>
    </Dialogo>
  );
}

/**
 * Quel che sta dentro il pannello, nelle due cornici: cambia con la fase.
 *
 * - **spenta**: cos'è, e il tasto che la accende;
 * - **libera**: «Ospita», i PC che ospitano con «Associa» o «Chiedi di
 *   unirti», e — se non se ne vede nessuno — le cose da controllare;
 * - **entrando**, **uscendo**: dove si è, e «Annulla» per chi sta entrando;
 * - **in sessione**: chi c'è, chi si può invitare e — per chi ospita —
 *   l'indirizzo da dettare, la sincronia, la calibrazione, e il tasto per
 *   uscire.
 *
 * In testa, in ogni fase, l'ultimo guasto e l'ultimo avviso: vedi
 * `NotizieNelPannello`. Stanno nella stessa posizione in tutte le fasi, così
 * un avviso a schermo non si rimonta — e non si ridice — quando la fase cambia
 * sotto di lui.
 *
 * `parla`: vedi `NotizieNelPannello`.
 */
function ContenutoInsieme({
  parla,
  onErrore,
}: {
  parla: boolean;
  onErrore: (e: unknown) => void;
}) {
  const stato = useStatoInsieme();
  if (stato === null) return null;
  const fase = stato.fase;
  const notizie = <NotizieNelPannello parla={parla} onErrore={onErrore} />;

  if (fase === "spenta") {
    return (
      <div className="insieme">
        {notizie}
        <p>{t("together.off")}</p>
        <div className="azioni-insieme">
          <button
            type="button"
            className="bottone primario"
            onClick={() => comanda(ipc.insiemeAttiva(true), onErrore)}
          >
            {t("together.turnOn")}
          </button>
        </div>
      </div>
    );
  }

  if (fase === "entrando" || fase === "uscendo") {
    // `aria-busy` sulla frase e non sul contenitore: sul contenitore teneva
    // zitte anche le notizie qui sopra, proprio mentre arriva «non ti ha
    // fatto entrare».
    return (
      <div className="insieme">
        {notizie}
        <p aria-busy="true">
          {fase === "uscendo"
            ? t("together.leaving")
            : stato.sessione === null
              ? t("together.entering")
              : t("together.entering.of", { nome: stato.sessione.host })}
        </p>
        {fase === "entrando" && (
          <div className="azioni-insieme">
            {/* Non c'è un comando per ritirare una richiesta: si esce, che per
                chi non è ancora dentro vuol dire proprio questo. */}
            <button type="button" className="bottone" onClick={() => esci(false, onErrore)}>
              {t("common.cancel")}
            </button>
          </div>
        )}
      </div>
    );
  }

  if (fase === "libera") {
    return (
      <div className="insieme">
        {notizie}
        <InAttesa dove="pannello" onErrore={onErrore} />
        <div className="azioni-insieme">
          <button type="button" className="bottone primario" onClick={() => ospita(onErrore)}>
            <Icona nome="i-insieme" dim={15} />
            {t("together.host")}
          </button>
        </div>
        <Vicini stato={stato} onErrore={onErrore} />
      </div>
    );
  }

  // In sessione.
  const ospito = fase === "ospita";
  return (
    <div className="insieme">
      {notizie}
      <InAttesa dove="pannello" onErrore={onErrore} />
      <Persone stato={stato} onErrore={onErrore} />
      {ospito && <DaInvitare stato={stato} onErrore={onErrore} />}
      {ospito && <IndirizziDiQui indirizzi={stato.indirizziDiQui} />}
      {!ospito && <Sincronia stato={stato} />}
      <Calibrazione
        ospito={ospito}
        calibrazione={stato.sessione?.calibrazione ?? false}
        onErrore={onErrore}
      />
      <p className="nota-insieme">
        {t(fraseProfiloRete(ospito))}
        {/* La porta da sola solo quando l'indirizzo intero non c'è: lì sopra
            la dice già, dopo i due punti. */}
        {ospito && stato.porta !== null && stato.indirizziDiQui.length === 0 && (
          <> {t("settings.together.port", { porta: String(stato.porta) })}</>
        )}
      </p>
      <div className="azioni-insieme">
        <button type="button" className="bottone" onClick={() => esci(ospito, onErrore)}>
          {ospito ? t("together.end") : t("together.leave")}
        </button>
      </div>
    </div>
  );
}

/**
 * Quanto si aspetta prima di dire cosa controllare, quando non si vede
 * nessuno: il giro di scoperta che parte all'apertura, uno di quelli normali
 * ogni dieci secondi, e un margine. Vedi `fraseNessunVicino`.
 */
const ATTESA_SCOPERTA = 15_000;

/** `true` quando sono passati `ms` da quando il componente c'è. */
function useTrascorso(ms: number): boolean {
  const [passato, setPassato] = useState(false);
  useEffect(() => {
    const conto = window.setTimeout(() => setPassato(true), ms);
    return () => window.clearTimeout(conto);
  }, [ms]);
  return passato;
}

/**
 * I PC che ospitano, o perché non se ne vede nessuno.
 *
 * Lo stesso componente nel pannello e nella scheda delle Impostazioni. Al
 * primo vero tentativo fra due PC la persona ha acceso la funzione dalle
 * Impostazioni e lì si è fermata: i PC che ospitano stavano solo nel pannello,
 * dietro un'icona di quindici pixel nella testata della colonna. Adesso li
 * trova dove ha acceso.
 *
 * Montato fa partire subito un giro di scoperta (`insieme_cerca`): chi lo
 * guarda sta cercando qualcuno adesso, e il giro normale arriva ogni dieci
 * secondi.
 *
 * Le righe hanno per chiave l'impronta e basta: col pari dentro la chiave,
 * associare un PC ne cambiava la chiave, React rifaceva la riga, e il fuoco
 * che stava sul suo «Associa» cadeva sul `<body>`. Così la riga resta, e il
 * tasto sotto il dito diventa «Chiedi di unirti».
 */
function Vicini({ stato, onErrore }: { stato: StatoInsieme; onErrore: (e: unknown) => void }) {
  const elenco = ospitanti(stato);
  const trascorso = useTrascorso(ATTESA_SCOPERTA);
  useEffect(() => {
    ipc.insiemeCerca().catch((e: unknown) => annota("cerca", e));
  }, []);
  return (
    <section className="gruppo-insieme">
      <h3>{t("together.nearby.title")}</h3>
      {elenco.length === 0 ? (
        <>
          <p>{t(fraseNessunVicino(trascorso, stato.indirizzi.length))}</p>
          {/* Chiuse di serie: le cause sono cinque righe, e servono solo a chi
              aspetta qualcuno che non arriva. */}
          <details className="cause-insieme">
            <summary>{t("together.causes.title")}</summary>
            <ul>
              {causeIrraggiungibile(stato.indirizzi.length).map((chiave) => (
                <li key={chiave}>{t(chiave)}</li>
              ))}
            </ul>
          </details>
        </>
      ) : (
        <ul className="elenco-insieme">
          {elenco.map((vicino) => {
            const nome = nomeDi(vicino.nome);
            return (
              <li key={vicino.impronta} className="riga-insieme">
                <span className="chi-insieme">
                  <span className="nome-insieme" dir="auto" title={nome}>
                    {nome}
                  </span>
                  {/* L'impronta, perché due PC possono chiamarsi tutti e due
                      «UFFICIO»: è quel che li distingue prima di associare. */}
                  <span
                    className="percorso"
                    title={t("together.nearby.fingerprint", { impronta: vicino.impronta })}
                  >
                    {vicino.impronta}
                  </span>
                </span>
                {vicino.bloccato ? (
                  <span className="etichetta-insieme">{t("together.nearby.blocked")}</span>
                ) : vicino.pari !== null ? (
                  <button
                    type="button"
                    className="bottone minuto primario"
                    aria-label={t("together.nearby.ask.aria", { nome })}
                    onClick={() => {
                      if (vicino.pari !== null) chiediDiEntrare(vicino.pari, onErrore);
                    }}
                  >
                    {t("together.nearby.ask")}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="bottone minuto btn-ghost"
                    aria-label={t("together.nearby.pair.aria", { nome })}
                    onClick={() => ipc.insiemeAssocia(vicino.chiave).catch(onErrore)}
                  >
                    {t("together.nearby.pair")}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/**
 * Chi c'è nella sessione. Chi ospita può allontanare gli altri.
 *
 * «Allontana» è della fase 2 nel piano, ma il comando c'è dalla fase 1 e il
 * gesto non dipende dai gesti inoltrati: è chi ospita che decide chi resta.
 */
function Persone({ stato, onErrore }: { stato: StatoInsieme; onErrore: (e: unknown) => void }) {
  const sessione = stato.sessione;
  if (sessione === null) return null;
  const mia = sessione.mia;
  return (
    <section className="gruppo-insieme">
      <h3>
        {t("together.people.title")}{" "}
        <span className="conto-insieme">{numero(sessione.persone.length)}</span>
      </h3>
      <ul className="elenco-insieme">
        {sessione.persone.map((persona) => (
          <li
            key={persona.membro}
            className={mia && !persona.io ? "riga-insieme con-diritti" : "riga-insieme"}
          >
            <span className="chi-insieme">
              <span className="nome-insieme" dir="auto" title={persona.nome}>
                {persona.nome}
              </span>
            </span>
            {persona.membro === 0 && (
              <span className="etichetta-insieme">{t("together.people.host")}</span>
            )}
            {persona.io && <span className="etichetta-insieme">{t("together.people.you")}</span>}
            {/* Chi segue vede il livello di ciascuno, chi ospita lo cambia.
                «Tutto» non si scrive: è com'era prima che ci fossero i livelli,
                e un'etichetta su ogni riga direbbe solo rumore. */}
            {!mia && persona.membro !== 0 && persona.diritti !== "tutto" && (
              <span className="etichetta-insieme">
                {persona.diritti === "ascolta"
                  ? t("together.rights.tag.listen")
                  : t("together.rights.tag.control")}
              </span>
            )}
            {mia && !persona.io && <LivelloDelMembro persona={persona} onErrore={onErrore} />}
            {mia && !persona.io && (
              <button
                type="button"
                className="bottone minuto btn-ghost"
                aria-label={t("together.people.remove.aria", { nome: persona.nome })}
                onClick={() => ipc.insiemeAllontana(persona.membro).catch(onErrore)}
              >
                {t("together.people.remove")}
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * Le tre linguette di «che cosa potrà fare»: Ascolta, Comanda, Tutto.
 *
 * Un componente solo per i tre posti in cui si sceglie — la richiesta, l'invito
 * e «In sessione» — perché le tre parole devono voler dire la stessa cosa
 * dappertutto. Il `title` sul guscio le spiega tutte e tre: le etichette sono
 * corte apposta, e chi non sa che cosa voglia dire «Comanda» lo legge
 * passandoci sopra. La spiegazione intera sta nella documentazione.
 */
function SceltaDiritti({
  nome,
  scelta,
  onScegli,
}: {
  nome: string;
  scelta: DirittiInsieme;
  onScegli: (diritti: DirittiInsieme) => void;
}) {
  return (
    <span className="diritti-insieme" title={t("together.rights.hint")}>
      <Segmentato
        voci={[
          { chiave: "ascolta", etichetta: t("together.rights.listen") },
          { chiave: "comanda", etichetta: t("together.rights.control") },
          { chiave: "tutto", etichetta: t("together.rights.all") },
        ]}
        scelta={scelta}
        onScegli={onScegli}
        etichetta={t("together.rights.aria", { nome })}
        classe="minuto"
      />
    </span>
  );
}

/** Il livello di chi sta per entrare, nell'archivio: vedi `VistaInsieme.livelli`. */
function LivelloDaScegliere({
  dove,
  id,
  nome,
}: {
  dove: "richieste" | "inviti";
  id: number;
  nome: string;
}) {
  const scelta = useLivello(dove, id);
  return (
    <SceltaDiritti nome={nome} scelta={scelta} onScegli={(d) => scegliLivello(dove, id, d)} />
  );
}

/**
 * Quanto si aspetta, dopo l'ultima linguetta scelta, prima di dirlo al nucleo.
 *
 * `Segmentato` sceglie a ogni freccia: da Ascolta a Tutto con la tastiera si
 * passa da Comanda, e mandarlo avrebbe dato a chi segue due avvisi e i tasti
 * che si accendono a metà. Quattrocento millisecondi bastano a chi preme le
 * frecce di seguito, e sono meno di quanto ci metta l'occhio ad accorgersi
 * dell'avviso dall'altra parte.
 */
const ATTESA_CONCEDI = 400;

/**
 * Chi ospita: il livello di un membro, da cambiare.
 *
 * La linguetta si accende subito al clic (`scelta`, ottimista) e resta accesa
 * finché il nucleo non lo conferma con lo stato: legata a `persona.diritti`
 * e basta, tornerebbe indietro per il tempo del viaggio. Il comando parte una
 * volta sola, quando la scelta si ferma (`ATTESA_CONCEDI`), e parte comunque
 * se il pannello si chiude prima: chiudere non è annullare.
 */
function LivelloDelMembro({
  persona,
  onErrore,
}: {
  persona: PersonaInsieme;
  onErrore: (e: unknown) => void;
}) {
  const [scelta, setScelta] = useState<DirittiInsieme | null>(null);
  const inSospeso = useRef<{ giro: number; diritti: DirittiInsieme } | null>(null);
  const membro = persona.membro;
  const errore = useRef(onErrore);
  errore.current = onErrore;
  const confermato = useRef(persona.diritti);
  confermato.current = persona.diritti;

  // Lo stato ha raggiunto la scelta: torna a comandare lui.
  useEffect(() => {
    if (scelta !== null && scelta === persona.diritti && inSospeso.current === null) {
      setScelta(null);
    }
  }, [persona.diritti, scelta]);

  // Chiudere il pannello a metà attesa manda subito quel che si è scelto.
  useEffect(
    () => () => {
      const sospeso = inSospeso.current;
      if (sospeso === null) return;
      window.clearTimeout(sospeso.giro);
      inSospeso.current = null;
      ipc.insiemeConcedi(membro, sospeso.diritti).catch((e: unknown) => errore.current(e));
    },
    [membro],
  );

  const scegli = (diritti: DirittiInsieme) => {
    setScelta(diritti);
    if (inSospeso.current !== null) window.clearTimeout(inSospeso.current.giro);
    const giro = window.setTimeout(() => {
      inSospeso.current = null;
      // Tornati al livello che c'era: niente stato in arrivo che lo confermi.
      if (diritti === confermato.current) setScelta(null);
      ipc.insiemeConcedi(membro, diritti).catch((e: unknown) => {
        setScelta(null);
        errore.current(e);
      });
    }, ATTESA_CONCEDI);
    inSospeso.current = { giro, diritti };
  };

  return (
    <SceltaDiritti nome={persona.nome} scelta={scelta ?? persona.diritti} onScegli={scegli} />
  );
}

/**
 * Chi ospita: i PC associati e collegati che non sono ancora dentro.
 *
 * Un invito mandato spegne il suo tasto per il minuto in cui vale, o finché il
 * nucleo non dice che non vale più: vedi `invita`. Nel pannello e nelle
 * Impostazioni, con lo stesso minuto.
 */
function DaInvitare({ stato, onErrore }: { stato: StatoInsieme; onErrore: (e: unknown) => void }) {
  const invitati = useArchivio(ARCHIVIO, (v) => v.invitati);
  const elenco = daInvitare(stato);
  return (
    <section className="gruppo-insieme">
      <h3>{t("together.invite.title")}</h3>
      {elenco.length === 0 ? (
        <p>{t("together.invite.none")}</p>
      ) : (
        <ul className="elenco-insieme">
          {elenco.map((fidato) => {
            const giaInvitato = invitati.has(fidato.id);
            return (
              <li key={fidato.id} className="riga-insieme con-diritti">
                <span className="chi-insieme">
                  <span className="nome-insieme" dir="auto" title={fidato.nome}>
                    {fidato.nome}
                  </span>
                </span>
                {!giaInvitato && (
                  <LivelloDaScegliere dove="inviti" id={fidato.id} nome={fidato.nome} />
                )}
                <button
                  type="button"
                  className="bottone minuto btn-ghost"
                  aria-label={t("together.invite.aria", { nome: fidato.nome })}
                  disabled={giaInvitato}
                  onClick={() =>
                    invita(
                      fidato.id,
                      ARCHIVIO.leggi().livelli.inviti.get(fidato.id) ?? DIRITTI_DI_PARTENZA,
                      onErrore,
                    )
                  }
                >
                  {giaInvitato ? t("together.invite.sent") : t("together.invite")}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/** Quanto resta scritto «Copiato» sul tasto, in millisecondi. */
const DURATA_COPIATO = 2_000;

/**
 * Dove si trova questo PC, per chi ospita: l'indirizzo da dettare a un collega
 * la cui rete non lascia passare la ricerca, con «Copia» accanto.
 *
 * Fino a qui l'unico modo di saperlo era aprire un prompt e leggere
 * `ipconfig`, e la porta la diceva una nota a parte: chi doveva scrivere
 * l'indirizzo dell'host sull'altro PC non sapeva né quale né dove. Il primo è
 * quello della strada di serie, il più probabile; gli altri — una seconda
 * scheda di rete, una VPN — stanno sotto «Altri indirizzi», chiusi: tutti in
 * fila farebbero scegliere a caso proprio chi non sa quale sia quello giusto.
 *
 * «Copia» è il primo tasto dell'albero che scrive negli appunti, quindi
 * l'idioma è nuovo e il più piccolo che basti: la scritta diventa «Copiato» per
 * due secondi, e la regione viva lo dice a chi ascolta lo schermo. Se gli
 * appunti rifiutano, l'indirizzo resta selezionabile e una riga dice di
 * copiarlo a mano.
 */
function IndirizziDiQui({ indirizzi }: { indirizzi: readonly string[] }) {
  const [esito, setEsito] = useState<{ indirizzo: string; riuscito: boolean } | null>(null);
  useEffect(() => {
    if (esito === null || !esito.riuscito) return;
    const conto = window.setTimeout(() => setEsito(null), DURATA_COPIATO);
    return () => window.clearTimeout(conto);
  }, [esito]);

  const [primo, ...altri] = indirizzi;
  if (primo === undefined) return null;

  const copia = (indirizzo: string) => {
    // Dentro una promessa anche la chiamata: senza appunti (`clipboard`
    // assente) l'errore è sincrono, e deve finire nello stesso ramo.
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(indirizzo))
      .then(
        () => setEsito({ indirizzo, riuscito: true }),
        () => setEsito({ indirizzo, riuscito: false }),
      );
  };
  const copiato = esito?.riuscito === true && esito.indirizzo === primo;

  return (
    <section className="gruppo-insieme">
      <Annuncio
        testo={
          esito === null
            ? null
            : esito.riuscito
              ? t("together.here.copied.aria", { indirizzo: esito.indirizzo })
              : t("together.here.copyFailed")
        }
      />
      <div className="indirizzo-insieme">
        <span className="chi-insieme">
          <Trans
            k="together.here.address"
            v={{ indirizzo: <strong className="percorso">{primo}</strong> }}
          />
        </span>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={t("together.here.copy.aria", { indirizzo: primo })}
          onClick={() => copia(primo)}
        >
          {copiato ? t("together.here.copied") : t("together.here.copy")}
        </button>
      </div>
      {esito !== null && !esito.riuscito && (
        <p className="nota-insieme">{t("together.here.copyFailed")}</p>
      )}
      <p className="nota-insieme">{t("together.here.hint")}</p>
      {altri.length > 0 && (
        <details className="altri-insieme">
          <summary>{t("together.here.others")}</summary>
          <ul>
            {altri.map((indirizzo) => (
              <li key={indirizzo} className="percorso">
                {indirizzo}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

/**
 * Chi segue: com'è agganciato all'host.
 *
 * Lo scarto si scrive solo a regime, e con il segno: «+3 ms» vuol dire che
 * questo PC è avanti. Prima dell'aggancio il numero oscilla di decine di
 * millisecondi per qualche secondo, e mostrarlo farebbe credere a un guasto
 * quel che è solo il regolatore al lavoro.
 */
function Sincronia({ stato }: { stato: StatoInsieme }) {
  const s = stato.sincronia;
  if (s === null) return null;
  const pezzi: string[] = [s.agganciato ? t("together.sync.locked") : t("together.sync.locking")];
  if (s.agganciato && s.erroreMs !== null) {
    const ms = numero(Math.round(s.erroreMs * 10) / 10, 1);
    pezzi.push(t("together.sync.offset", { ms: s.erroreMs > 0 ? `+${ms}` : ms }));
    if (s.incertezzaMs !== null) {
      pezzi.push(t("together.sync.uncertainty", { ms: numero(Math.round(s.incertezzaMs * 10) / 10, 1) }));
    }
  }
  return (
    <p className="nota-insieme" role="status">
      {pezzi.join(" · ")}
    </p>
  );
}

/**
 * La calibrazione: i clic su tutti i PC, e il cursore della latenza di questo.
 *
 * I clic li fa partire chi ospita; il cursore ce l'ha ognuno, perché la
 * latenza da correggere è quella della **propria** uscita — le casse di
 * questo PC, le cuffie di quello — e si salva su quella scheda audio. Il
 * criterio sta scritto qui, accanto al cursore, perché è l'unica istruzione
 * che serve e la si legge mentre si ascolta: «dal centro», e non «finché i
 * clic si fondono», che per l'effetto di precedenza succede già con qualche
 * millisecondo di scarto e lascerebbe la correzione a metà.
 */
function Calibrazione({
  ospito,
  calibrazione,
  onErrore,
}: {
  ospito: boolean;
  /** I clic stanno suonando, secondo il nucleo: per chi ospita e per chi segue. */
  calibrazione: boolean;
  onErrore: (e: unknown) => void;
}) {
  const idTitolo = useId();
  return (
    <section className="gruppo-insieme calibra-insieme" aria-labelledby={idTitolo}>
      <h3 id={idTitolo}>{t("together.calibrate.title")}</h3>
      {ospito && (
        <div className="azioni-insieme">
          <button
            type="button"
            className="bottone"
            aria-pressed={calibrazione}
            onClick={() => calibra(!calibrazione, onErrore)}
          >
            {calibrazione ? t("together.calibrate.stop") : t("together.calibrate.start")}
          </button>
        </div>
      )}
      {/* Chi segue non ha il tasto, e senza questa riga non saprebbe che i
          clic che sente sono quelli da usare: è il momento di muovere il suo
          cursore. */}
      {!ospito && calibrazione && <p role="status">{t("together.calibrate.running")}</p>}
      <p>
        <Trans
          k="together.calibrate.how"
          v={{ centro: <strong>{t("together.calibrate.center")}</strong> }}
        />
      </p>
      <p>{t("together.calibrate.bluetooth")}</p>
      <CursoreLatenza
        limite={LIMITE_CALIBRAZIONE_MS}
        etichetta={t("settings.output.latency.title")}
        conPassi
        onErrore={onErrore}
      />
      <p className="nota-insieme">
        {ospito ? t("together.calibrate.host") : t("together.calibrate.member")}
      </p>
    </section>
  );
}

// ── la scheda delle Impostazioni ───────────────────────────────────────────

/** Lo stato di un PC associato, in due parole. */
function statoDelFidato(fidato: FidatoInsieme): string {
  if (fidato.bloccato) return t("settings.together.trusted.blocked");
  if (fidato.inSessione) return t("settings.together.trusted.inSession");
  if (fidato.ospita) return t("settings.together.trusted.hosting");
  if (fidato.presente) return t("settings.together.trusted.online");
  return fidato.vistoMs !== null
    ? t("settings.together.trusted.seen", { quando: dataOraBreve(fidato.vistoMs) })
    : t("settings.together.trusted.never");
}

/**
 * La scheda «Ascolta insieme» delle Impostazioni: l'interruttore, il nome di
 * questo PC, «Ospita» e il tasto che apre il pannello, i PC che ospitano — o,
 * per chi ospita, l'indirizzo di questo PC e chi si può invitare — i PC
 * associati, gli indirizzi scritti a mano.
 *
 * I PC che ospitano e «Invita» sono gli stessi componenti del pannello
 * (`Vicini`, `DaInvitare`, `IndirizziDiQui`), dentro il suo stesso vestito
 * (`.insieme`): è qui che si accende la funzione, ed è qui che chi l'ha appena
 * accesa cerca con chi ascoltare. Una copia avrebbe avuto i suoi difetti.
 *
 * Si gestisce da sé per la ragione di `Aggiornamenti` e `Scrobbling`: è uno
 * stato che nessun altro della schermata guarda, e passarlo da `App` vorrebbe
 * dire allungare di cinque le prop di `Impostazioni` per una scheda che si
 * apre di rado. Lo legge dall'archivio di questo file, quindi dice la stessa
 * cosa del pannello nello stesso istante.
 *
 * I PC associati si toccano anche a funzione spenta: rinominare, bloccare e
 * dimenticare sono righe del database di questo PC, e chi spegne tutto per
 * fare ordine deve poterlo fare.
 */
export function SchedaInsieme({ onErrore }: { onErrore: (e: unknown) => void }) {
  const stato = useStatoInsieme();
  // `null` vuol dire «non toccato»: il campo mostra il nome del nucleo, e una
  // risposta che lo cambia — un altro posto che lo rinomina — si vede subito.
  const [nome, setNome] = useState<string | null>(null);
  const [nomi, setNomi] = useState<Record<number, string>>({});
  const [nuovo, setNuovo] = useState("");
  const [guastoIndirizzo, setGuastoIndirizzo] = useState<Chiave | null>(null);
  const idNome = useId();
  const idIndirizzi = useId();
  const idGuasto = useId();

  if (stato === null) {
    return (
      <Interruttore
        etichetta={t("settings.together.toggle")}
        acceso={false}
        impedito={t("settings.together.unavailable")}
      />
    );
  }

  const nomeScritto = nome ?? stato.nome;
  const salvaNome = () => {
    if (nome === null) return;
    comanda(ipc.insiemeNome(nomePulito(nome)), onErrore);
    setNome(null);
  };

  const rinomina = (fidato: FidatoInsieme) => {
    const scritto = nomi[fidato.id];
    if (scritto === undefined) return;
    comanda(ipc.insiemePariRinomina(fidato.id, nomePulito(scritto)), onErrore);
    setNomi(({ [fidato.id]: _tolto, ...resto }) => resto);
  };

  const aggiungi = () => {
    const esito = aggiungiIndirizzo(stato.indirizzi, nuovo);
    if (esito.elenco === null) {
      setGuastoIndirizzo(esito.errore);
      return;
    }
    setGuastoIndirizzo(null);
    setNuovo("");
    comanda(ipc.insiemeIndirizzi(esito.elenco), onErrore);
  };

  const togliIndirizzo = (indirizzo: string) =>
    comanda(
      ipc.insiemeIndirizzi(stato.indirizzi.filter((i) => i !== indirizzo)),
      onErrore,
    );

  const fase = stato.fase;
  let tastoSessione: ReactNode;
  if (fase === "ospita" || fase === "membro") {
    tastoSessione = (
      <button
        type="button"
        className="bottone btn-ghost"
        onClick={() => esci(fase === "ospita", onErrore)}
      >
        {fase === "ospita" ? t("together.end") : t("together.leave")}
      </button>
    );
  } else {
    tastoSessione = (
      <button
        type="button"
        className="bottone btn-ghost"
        disabled={fase !== "libera"}
        title={stato.attivo ? undefined : t("together.host.off")}
        onClick={() => ospita(onErrore)}
      >
        <Icona nome="i-insieme" dim={15} />
        {t("together.host")}
      </button>
    );
  }

  return (
    <>
      <Interruttore
        etichetta={t("settings.together.toggle")}
        spiegazione={t("settings.together.toggle.hint")}
        acceso={stato.attivo}
        onCambia={(attivo) => comanda(ipc.insiemeAttiva(attivo), onErrore)}
      />

      <h3 className="titoletto" id={idNome}>
        {t("settings.together.name")}
      </h3>
      <div className="azioni">
        <input
          type="text"
          className="campo field-input"
          aria-labelledby={idNome}
          maxLength={MASSIMO_NOME}
          value={nomeScritto}
          onChange={(e) => setNome(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") salvaNome();
          }}
        />
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={nome === null || nomePulito(nome) === stato.nome}
          onClick={salvaNome}
        >
          {t("common.save")}
        </button>
      </div>
      <p className="nota">{t("settings.together.name.hint")}</p>

      <div className="azioni">
        {tastoSessione}
        {/* Il pannello è dove succede il resto — chi c'è, la calibrazione, gli
            inviti che arrivano — e fino a qui dalle Impostazioni non c'era una
            strada per arrivarci. Anche a funzione spenta: il pannello dice
            cos'è e la accende. */}
        <button type="button" className="bottone btn-ghost" onClick={apriFinestra}>
          {t("settings.together.openPanel")}
        </button>
      </div>
      {fase === "ospita" && stato.porta !== null && stato.indirizziDiQui.length === 0 && (
        <p className="nota">{t("settings.together.port", { porta: String(stato.porta) })}</p>
      )}
      {stato.attivo && (fase === "libera" || fase === "ospita") && (
        <div className="insieme">
          {fase === "libera" ? (
            <Vicini stato={stato} onErrore={onErrore} />
          ) : (
            <>
              <IndirizziDiQui indirizzi={stato.indirizziDiQui} />
              <DaInvitare stato={stato} onErrore={onErrore} />
            </>
          )}
        </div>
      )}

      <h3 className="titoletto">{t("settings.together.trusted")}</h3>
      {stato.fidati.length === 0 ? (
        <p className="nota">{t("settings.together.trusted.none")}</p>
      ) : (
        <ul className="cartelle">
          {ordinaFidati(stato.fidati).map((fidato) => {
            const scritto = nomi[fidato.id];
            return (
              <li key={fidato.id} className="azioni">
                <input
                  type="text"
                  className="campo field-input"
                  aria-label={t("settings.together.trusted.name", { nome: fidato.nome })}
                  maxLength={MASSIMO_NOME}
                  value={scritto ?? fidato.nome}
                  onChange={(e) => setNomi({ ...nomi, [fidato.id]: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") rinomina(fidato);
                  }}
                />
                <span
                  className="percorso"
                  title={t("together.nearby.fingerprint", { impronta: fidato.impronta })}
                >
                  {fidato.impronta} · {statoDelFidato(fidato)}
                </span>
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={
                    scritto === undefined ||
                    nomePulito(scritto) === "" ||
                    nomePulito(scritto) === fidato.nome
                  }
                  onClick={() => rinomina(fidato)}
                >
                  {t("settings.together.trusted.rename")}
                </button>
                <button
                  type="button"
                  className="bottone btn-ghost"
                  aria-pressed={fidato.bloccato}
                  onClick={() =>
                    comanda(ipc.insiemePariBlocca(fidato.id, !fidato.bloccato), onErrore)
                  }
                >
                  {fidato.bloccato
                    ? t("settings.together.trusted.unblock")
                    : t("settings.together.trusted.block")}
                </button>
                {/* Senza domanda, come «Dimentica» dei dispositivi della
                    sincronia qui sopra: si rimedia riassociando, con il
                    codice, e non si perde niente che non torni. */}
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={() => comanda(ipc.insiemePariDimentica(fidato.id), onErrore)}
                >
                  {t("settings.together.trusted.forget")}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <h3 className="titoletto" id={idIndirizzi}>
        {t("settings.together.addresses")}
      </h3>
      <p className="nota">{t("settings.together.addresses.hint")}</p>
      {stato.indirizzi.length > 0 && (
        <ul className="cartelle" aria-labelledby={idIndirizzi}>
          {stato.indirizzi.map((indirizzo) => (
            <li key={indirizzo} className="azioni">
              <span className="percorso">{indirizzo}</span>
              <button
                type="button"
                className="tasto icon-btn"
                aria-label={t("settings.together.addresses.remove", { indirizzo })}
                title={t("settings.together.addresses.remove", { indirizzo })}
                onClick={() => togliIndirizzo(indirizzo)}
              >
                <Icona nome="i-x" dim={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="azioni">
        <input
          type="text"
          className="campo field-input"
          aria-label={t("settings.together.addresses.new")}
          aria-invalid={guastoIndirizzo !== null || undefined}
          aria-describedby={guastoIndirizzo !== null ? idGuasto : undefined}
          // Un esempio e non una frase: un indirizzo si legge uguale in ogni
          // lingua, e la forma è proprio quel che deve insegnare.
          placeholder="192.168.1.20:47811"
          value={nuovo}
          onChange={(e) => {
            setNuovo(e.target.value);
            setGuastoIndirizzo(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") aggiungi();
          }}
        />
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={nuovo.trim() === "" || stato.indirizzi.length >= MASSIMO_INDIRIZZI}
          onClick={aggiungi}
        >
          {t("settings.together.addresses.add")}
        </button>
      </div>
      {guastoIndirizzo !== null && (
        <p className="nota" id={idGuasto} role="alert">
          {t(guastoIndirizzo)}
        </p>
      )}
    </>
  );
}
