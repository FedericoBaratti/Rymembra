/**
 * Impostazioni: tutto quel che prima stava nella barra laterale.
 *
 * # Perché è una pagina e non un pannello
 *
 * Cartelle, scansione, importazione, skin e statistiche vivevano in cinque
 * blocchi impilati sotto la navigazione. Non era una scelta di disegno: era il
 * posto dove c'era spazio. Il risultato è che la barra che dice «dove sono»
 * conteneva anche una barra di avanzamento, due menù a tendina e un pulsante che
 * riscrive i nomi dei file sul disco — comandi rari, permanenti, e alla stessa
 * distanza dal dito di «Album».
 *
 * Qui sono sei sezioni con un indice a sinistra, e la larghezza per spiegare
 * cosa fanno. Un comando che sposta dei file merita una riga di testo accanto;
 * in una colonna da 240 pixel quella riga non ci stava, quindi non c'era.
 */
import {
  Fragment,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";

import type {
  Avanzamento,
  AvanzamentoArricchimento,
  AvanzamentoTesti,
  AvanzamentoNuvola,
  AvanzamentoVolume,
  Avvio,
  EsitoArricchimento,
  EsitoScansione,
  ModoLivello,
  Normalizzazione,
  QuadroSorveglianza,
  StatoArricchimento,
  Resoconto,
  SaluteDatabase,
  StatoManutenzione,
  StatoNuvola,
  StatoSincronia,
  StatoTesti,
  VoceSkin,
} from "../ipc";
import {
  DISSOLVENZA_MASSIMA_S,
  FINE_DEL_BRANO,
  ipc,
  testoErrore,
} from "../ipc";
import { dataOra, durata, numero, ore } from "../formato";
import { DISPONIBILI, t, tSe, type Chiave } from "../lingue";
import { useAscolto } from "../pagine";
import { Aggiornamenti } from "../parti/Aggiornamenti";
import { Equalizzatore } from "../parti/Equalizzatore";
import { Icona, type NomeIcona } from "../parti/Icone";
import { Interruttore } from "../parti/Interruttore";
import { ModelliIA } from "../parti/ModelliIA";
import type { UsoImportazioni } from "../parti/Importazioni";
import type { Vista } from "../parti/Navigazione";
import { Profilo } from "../parti/Profilo";
import { Scorciatoie } from "../parti/Scorciatoie";
import { Scrobbling } from "../parti/Scrobbling";
import { SchedaSpettro } from "../parti/SchedaSpettro";
import { SchedaUscita } from "../parti/UscitaAudio";
import { Segmentato } from "../parti/Segmentato";
import type { Associazioni } from "../tastiera";
import type { MovimentoUtente } from "../aspetto";
import type { Tema } from "../tema";
import { Trans } from "../lingue/Trans";
import { nomeFonte } from "../parti/Incertezza";

/** Le tredici sezioni, nell'ordine in cui si visitano la prima volta. */
export type Sezione =
  | "cartelle"
  | "aspetto"
  | "modelli"
  | "chiusura"
  | "riproduzione"
  | "movimento"
  | "nuvola"
  | "sincronia"
  | "scrobbling"
  | "esterno"
  | "legacy"
  | "dati"
  | "aggiornamenti";

/**
 * L'indice, a gruppi.
 *
 * # Perché a gruppi e non più di fila
 *
 * Perché tredici voci in colonna non sono un indice: sono un elenco che si
 * rilegge dall'alto ogni volta, e la sola cosa che vi si trova a colpo d'occhio
 * è quella che si era già trovata l'ultima volta. Le intestazioni non tolgono
 * niente — le sezioni restano quelle, con dentro le stesse schede — e danno
 * quattro punti di appoggio invece di uno.
 *
 * # Cosa sta con cosa
 *
 * **Preferenze** è quel che si decide una volta e vale sempre: dove sta la
 * musica, come si vede il programma, con quale modello parla, e cosa fa la X.
 * Sono le quattro sezioni in cui non succede niente — nessuna passata, nessun
 * trasferimento, nessun account: si sceglie e si chiude.
 *
 * **Ascolto** è quel che tocca il suono mentre suona.
 *
 * **Servizi** sono i posti fuori da questo computer, e la domanda che li unisce
 * è una sola: cosa esce, e verso chi. Backup su Drive, sincronia fra
 * dispositivi, scrobbling, importazione da un servizio, e l'importazione dalla
 * versione precedente — che è la stessa cosa da vicinissimo.
 *
 * **Il programma** è Aether che parla di sé.
 *
 * # «Modelli IA» sotto Preferenze
 *
 * Stava dopo «Scrobbling», e la ragione scritta qui era buona: sono le tre
 * schede di fila che parlano di un servizio con cui Aether scambia qualcosa, e
 * i modelli sono l'unica impostazione di tutto il programma che possa far
 * uscire da questo computer una cosa **scritta dall'utente**.
 *
 * Quel che quell'ordine difendeva non era però la vicinanza: era che la
 * domanda «cosa esce, e verso chi» si leggesse. Un indice non è il posto dove
 * leggerla — ci si passa di corsa — e adesso sta dove serve, dentro la scheda
 * dei modelli, che è dove si incolla la chiave. La vicinanza da sola non ha
 * mai detto niente a nessuno.
 *
 * # `esterno` e non `spotify`
 *
 * Spotify è **una** delle sorgenti, e l'etichetta lo dice adesso quanto la
 * chiave. Si chiamava «Da un link», che era il nome di **una** delle due
 * strade — l'account intero stava nella scheda accanto — e prometteva quindi
 * metà di quel che la sezione fa.
 */
function gruppi(): readonly (readonly [
  string,
  readonly (readonly [Sezione, string, NomeIcona])[],
])[] {
  return [
    [
      t("settings.group.preferenze"),
      [
        ["cartelle", t("settings.section.cartelle"), "i-folder"],
        ["aspetto", t("settings.section.aspetto"), "i-skin"],
        ["modelli", t("settings.section.modelli"), "i-ia"],
        ["chiusura", t("settings.section.chiusura"), "i-x"],
      ],
    ],
    [
      t("settings.group.ascolto"),
      [
        ["riproduzione", t("settings.section.riproduzione"), "i-play"],
        ["movimento", t("settings.section.movimento"), "i-eq"],
      ],
    ],
    [
      t("settings.group.servizi"),
      [
        ["nuvola", t("settings.section.nuvola"), "i-cloud"],
        ["sincronia", t("settings.section.sincronia"), "i-cloud"],
        ["scrobbling", t("settings.section.scrobbling"), "i-cloud"],
        ["esterno", t("settings.section.esterno"), "i-list"],
        ["legacy", t("settings.section.legacy"), "i-import"],
      ],
    ],
    [
      t("settings.group.programma"),
      [
        ["dati", t("settings.section.dati"), "i-album"],
        // Per ultima, e non perché avanzasse: è l'unica sezione che parla di
        // qualcosa che il programma fa **senza** che nessuno gliel'abbia
        // chiesto, e chi la cerca la cerca apposta. Metterla in alto vorrebbe
        // dire darle il posto di «Cartelle», che è invece la prima cosa che
        // serve a chiunque.
        ["aggiornamenti", t("settings.section.aggiornamenti"), "i-cloud"],
      ],
    ],
  ];
}

/**
 * Le sezioni spianate, senza i gruppi.
 *
 * Serve a chi cerca una sezione per chiave — la ricerca, per l'icona e per il
 * «dove» — e non a chi disegna l'indice. Una funzione invece di due `flatMap`
 * scritti nei due punti: sono lo stesso elenco, e due copie sono due occasioni
 * di aggiungere una sezione a una sola delle due.
 */
function sezioni(): readonly (readonly [Sezione, string, NomeIcona])[] {
  return gruppi().flatMap(([, dentro]) => dentro);
}

/**
 * Cosa c'è dentro le sezioni, per poterlo cercare.
 *
 * # Perché un indice scritto a mano e non il testo della pagina
 *
 * Cercare dentro il DOM troverebbe soltanto la sezione **aperta**: le altre
 * otto non sono disegnate, e un motore di ricerca che vede un nono di quel che
 * c'è è peggio di nessuno — dice «non c'è» di cose che ci sono.
 *
 * I `sinonimi` sono la metà che conta. Nessuno cerca «normalizzazione»: si
 * cerca «volume», «uguale», «replaygain». Nessuno cerca «arricchimento»: si
 * cerca «copertine» o «tag». La riga è un elenco di come la gente chiama la
 * cosa, non di come l'abbiamo chiamata noi.
 *
 * # Perché i sinonimi stanno nel catalogo delle lingue
 *
 * Perché sono la ricerca, non una decorazione: una traduzione che porta i
 * titoli e lascia i sinonimi in italiano dà un campo di ricerca che in inglese
 * trova un terzo di quel che dovrebbe, e nessuno collega le due cose. Nel file
 * di lingua stanno come una riga sola separata da virgole — chi traduce ne
 * aggiunge quanti ne servono nella sua lingua, che sono quasi sempre un numero
 * diverso.
 */
function voci(): readonly {
  sezione: Sezione;
  titolo: string;
  sinonimi: readonly string[];
}[] {
  const v = (sezione: Sezione, nome: string) => ({
    sezione,
    titolo: t(`settings.entry.${nome}` as Chiave),
    sinonimi: t(`settings.entry.${nome}.syn` as Chiave).split(","),
  });
  return [
    v("cartelle", "roots"),
    v("cartelle", "scan"),
    v("cartelle", "downloads"),
    v("aspetto", "language"),
    v("aspetto", "theme"),
    v("aspetto", "skin"),
    v("aspetto", "accent"),
    // Nessuno cerca «onboarding»: si cerca «giro», «tour», «guida», «aiuto».
    v("aspetto", "tour"),
    v("riproduzione", "output"),
    v("riproduzione", "eq"),
    v("riproduzione", "replaygain"),
    v("riproduzione", "sleep"),
    v("riproduzione", "autoplay"),
    v("riproduzione", "crossfade"),
    v("riproduzione", "spectrum"),
    // Nessuno cerca «dati tecnici»: si cerca «bitrate», «flac», «kbps», o «khz»
    // — cioè quel che c'è scritto nella riga, non come l'abbiamo intitolata.
    // I sinonimi qui sono la voce più del titolo.
    v("riproduzione", "fileFormat"),
    v("riproduzione", "enrich"),
    v("movimento", "motion"),
    v("movimento", "shortcuts"),
    // Nessuno cerca «zoom» sapendo che si chiama così: si cerca «più
    // grande», «testo piccolo», o il nome del tasto. E la si cerca proprio
    // quando non si riesce a leggere, cioè nel momento in cui trovarla conta
    // di più: una scheda che l'indice non conosce sarebbe raggiungibile solo
    // dalle scorciatoie, che è esattamente quel che può mancare.
    v("movimento", "zoom"),
    v("nuvola", "drive"),
    v("sincronia", "sync"),
    v("sincronia", "devices"),
    v("legacy", "legacy"),
    v("esterno", "link"),
    v("esterno", "account"),
    // La pagina non è una sezione delle impostazioni, ma è quel che si cerca
    // scrivendo «coda»: la voce ci porta, il rimando dentro la scheda la apre.
    v("esterno", "importQueue"),
    v("scrobbling", "scrobbling"),
    v("modelli", "models"),
    // «Chiave API» è una voce sua e non un sinonimo: chi la cerca non sta
    // cercando un modello — sta cercando dove si incolla il segreto, e una riga
    // intitolata «Modelli IA» non gli dice che è lì dentro.
    v("modelli", "apikey"),
    // Nessuno cerca «secondo piano»: si cerca «chiudi», «x», «tray», o
    // «continua a suonare». I sinonimi qui sono la voce, più del titolo.
    v("chiusura", "background"),
    v("dati", "numbers"),
    v("dati", "profile"),
    // Nessuno cerca «manutenzione»: si cerca «spazio», «cache», o «pulisci» —
    // di solito il giorno in cui il disco è pieno.
    v("dati", "covers"),
    v("aggiornamenti", "update"),
    // «Versione» è una voce sua e non un sinonimo di «aggiornamenti»: chi la
    // cerca quasi sempre non vuole aggiornare niente — vuole sapere quale
    // numero scrivere in una segnalazione — e trovare una riga intitolata
    // «Aggiornamenti» non gli dice che il numero è lì dentro.
    v("aggiornamenti", "version"),
  ];
}

/** Le voci che una ricerca trova. Vuota la ricerca, nessuna. */
function cerca(testo: string): ReturnType<typeof voci> {
  const q = testo.trim().toLowerCase();
  if (q === "") return [];
  const dove = sezioni();
  return voci().filter(
    (v) =>
      v.titolo.toLowerCase().includes(q) ||
      v.sinonimi.some((s) => s.trim().toLowerCase().includes(q)) ||
      (dove.find(([chiave]) => chiave === v.sezione)?.[1] ?? "")
        .toLowerCase()
        .includes(q),
  );
}

/** Quando è successa una cosa, in una forma che si legge a colpo d'occhio. */
function quando(ms: number | null): string {
  if (ms === null || ms <= 0) return t("settings.when.never");
  const passati = Date.now() - ms;
  if (passati < 60_000) return t("settings.when.justNow");
  if (passati < 3_600_000)
    return t("settings.when.minutesAgo", { n: Math.round(passati / 60_000) });
  return dataOra(ms);
}

/**
 * Cosa è cambiato nell'ultima passata, in una frase.
 *
 * È l'unica cosa che rende leggibile un automatismo che scrive nella libreria da
 * solo. «Sincronizzato» non dice niente; «142 ascolti, 3 playlist» dice se è
 * successo quel che ci si aspettava — e, quando non lo è, che è il momento di
 * guardare.
 *
 * Nessuna voce a zero: un elenco di sei numeri di cui cinque sono zero fa
 * lavorare chi legge per trovare l'unico che conta.
 */
function riassunto(r: Resoconto): string {
  const pezzi: string[] = [];
  const conta = (quanti: number, cosa: string) => {
    if (quanti > 0) {
      pezzi.push(
        `${numero(quanti)} ${t(`sync.count.${cosa}` as Chiave, { n: quanti })}`,
      );
    }
  };
  conta(r.cambiamenti.ascolti, "plays");
  conta(r.cambiamenti.voti, "ratings");
  conta(r.cambiamenti.preferiti, "likes");
  conta(r.cambiamenti.posizioni, "positions");
  conta(r.cambiamenti.playlist, "playlists");
  // `playlist_tolte` e non `playlistTolte`: il `rename_all` del Rust sta sul
  // `Resoconto` esterno e non scende nel tipo annidato — vedi la nota in
  // `ipc.ts`. Prima di questa release qui arrivava sempre `undefined`.
  conta(r.cambiamenti.playlist_tolte, "playlistsGone");
  conta(r.cambiamenti.cartelle, "folders");
  if (pezzi.length === 0) {
    // Una passata a vuoto va detta, e va detta come una cosa normale: è la
    // condizione in cui la sincronia passa la maggior parte del suo tempo.
    return r.letti > 0
      ? t("sync.summary.readAligned", { n: r.letti })
      : t("sync.summary.aligned");
  }
  return t("sync.summary.arrived", { pezzi: pezzi.join(", ") });
}

/** Come si chiamano le tre fasi di un salvataggio, a schermo. */
function fasiNuvola(): Record<AvanzamentoNuvola["cosa"], string> {
  return {
    metadati: t("cloud.phase.metadati"),
    skin: t("cloud.phase.skin"),
    bozze: t("cloud.phase.bozze"),
  };
}

/**
 * L'avanzamento di un salvataggio su Drive.
 *
 * L'evento partiva da sempre e lo ascoltava soltanto la finestrella del
 * ripristino: un «Salva adesso» da questa pagina mostrava «Salvataggio in
 * corso…» sul pulsante e nient'altro, per tutto il tempo. Su una libreria
 * grande sono decine di secondi identici a un blocco.
 *
 * Si azzera quando il salvataggio finisce — `inCorso` torna falso — e non
 * quando arriva l'ultimo passo: l'ultimo passo di una fase non è la fine del
 * lavoro, e una barra ferma al 100% mentre il caricamento continua racconta
 * qualcosa che non è successo.
 */
function useAvanzamentoNuvola(inCorso: boolean): AvanzamentoNuvola | null {
  const [avanzamento, setAvanzamento] = useState<AvanzamentoNuvola | null>(null);

  useAscolto<AvanzamentoNuvola>("nuvola:avanzamento", setAvanzamento);

  useEffect(() => {
    if (!inCorso) setAvanzamento(null);
  }, [inCorso]);

  return inCorso ? avanzamento : null;
}

/**
 * Dove va un errore del «?» di una scheda.
 *
 * Una `Scheda` è disegnata da una dozzina di componenti diversi, e nessuno di
 * loro ha motivo di passarle `onErrore` solo per il pulsante dell'intestazione:
 * lo mette `Impostazioni` una volta, in cima, e chi lo vuole lo legge da qui.
 */
const ErroreGuida = createContext<(e: unknown) => void>(() => {});

/**
 * Una scheda di sezione.
 *
 * Le spiegazioni non stanno qui: stanno nella documentazione tecnica sul sito,
 * e il «?» dell'intestazione apre la voce che spiega come funziona questa parte
 * (`guida` è l'ancora, e l'elenco di quelle ammesse sta di là, in `comandi.rs`). `nota` resta per un
 * dato che cambia — «3 skin installate» — e non per una frase che spiega.
 */
function Scheda({
  icona,
  titolo,
  nota,
  guida,
  children,
}: {
  icona: NomeIcona;
  titolo: string;
  nota?: string | undefined;
  guida?: string | undefined;
  children: React.ReactNode;
}) {
  const onErrore = useContext(ErroreGuida);
  return (
    <section className="scheda section-card">
      <header>
        <span className="section-icon" aria-hidden="true">
          <Icona nome={icona} dim={16} />
        </span>
        <h2 className="section-heading">{titolo}</h2>
        {nota !== undefined && <span className="nota-testa">{nota}</span>}
        {guida !== undefined && (
          <button
            type="button"
            className="guida-scheda"
            title={t("settings.help")}
            aria-label={t("settings.help.of", { titolo })}
            onClick={() => {
              ipc.apriGuida(guida).catch(onErrore);
            }}
          >
            ?
          </button>
        )}
      </header>
      {children}
    </section>
  );
}

/**
 * «Rileggi tutti i tag»: la prossima scansione rilegge ogni file, e parte subito.
 *
 * # A cosa serve
 *
 * La scansione rilegge un file quando ne cambiano la data o la dimensione. Ci
 * sono ritocchi che non cambiano né l'una né l'altra — un taggatore che ferma
 * la data e riscrive nello spazio libero del tag — e righe scritte da una
 * versione che leggeva meno: questo è il modo di dire «guardali tutti».
 * Correzioni a mano e metadati trovati dall'arricchimento sopravvivono alla
 * rilettura come a ogni altra.
 *
 * # Perché avvia la scansione di sempre
 *
 * Il nucleo segna soltanto; a leggere è `onScansiona`, cioè la stessa scansione
 * del pulsante accanto, con il suo avanzamento, il suo Annulla e le sue
 * conferme. Una seconda strada per leggere i file sarebbe una seconda cosa da
 * tenere d'accordo con la prima.
 */
function RileggiTag({
  occupato,
  onScansiona,
  onErrore,
}: {
  occupato: boolean;
  onScansiona: () => void;
  onErrore: (e: unknown) => void;
}) {
  const [segnati, setSegnati] = useState<number | null>(null);
  return (
    <>
      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={occupato}
          onClick={() => {
            ipc
              .metadatiRileggiTutti()
              .then((quanti) => {
                setSegnati(quanti);
                onScansiona();
              })
              .catch(onErrore);
          }}
        >
          <Icona nome="i-scan" dim={15} />
          {t("settings.roots.reread")}
        </button>
        {segnati !== null && (
          <span className="oppure">
            {t("settings.roots.reread.marked", { n: segnati })}
          </span>
        )}
      </div>
    </>
  );
}

/**
 * La sorveglianza delle cartelle: i due interruttori, e cosa sta succedendo.
 *
 * # Perché si gestisce da sé
 *
 * Per la stessa ragione di `SchedaTesti`: nessun'altra parte del programma
 * guarda questo stato, e farlo scendere da `App` vorrebbe dire altre prop per
 * una riga che si legge solo qui. Lo chiede al montaggio — è un quadro in
 * memoria, non tocca né disco né database — e lo riceve aggiornato da ogni
 * cambio di scelta.
 *
 * # Due interruttori per tre scelte
 *
 * Il nucleo ha tre modi — spenta, dischi locali, anche la rete — e la
 * finestra li mostra come due domande, perché sono due decisioni diverse:
 * «voglio che la libreria si aggiorni da sola?» e «anche a costo di tenere
 * sveglio il NAS?». La seconda senza la prima non ha senso, e lo dice.
 */
function SorveglianzaCartelle({
  onErrore,
}: {
  /** La scelta scrive nel database: se non ci arriva, va detto. */
  onErrore: (e: unknown) => void;
}) {
  const [quadro, setQuadro] = useState<QuadroSorveglianza | null>(null);

  useEffect(() => {
    let annullato = false;
    ipc
      .sorveglianzaStato()
      .then((letto) => {
        if (!annullato) setQuadro(letto);
      })
      .catch(() => {
        /* Senza quadro gli interruttori restano com'è di serie: niente di
           distruttivo da annunciare. */
      });
    return () => {
      annullato = true;
    };
  }, []);

  const modo = quadro?.modo ?? "locali";
  const scegli = (scelta: QuadroSorveglianza["modo"]) => {
    ipc.impostaSorveglianza(scelta).then(setQuadro).catch(onErrore);
  };
  const quante = (come: string) =>
    quadro?.cartelle.filter((c) => c.come === come).length ?? 0;
  const ferme = quante("nonRaggiungibile");
  // Con «solo i dischi di questo computer» le cartelle di rete sono `ferma`:
  // si contano anche loro, se no il NAS spariva dai conteggi e sembrava che la
  // sorveglianza lo seguisse.
  const diRete = modo === "locali" ? quante("ferma") : 0;

  return (
    <>
      <Interruttore
        etichetta={t("settings.roots.watch")}
        acceso={modo !== "spenta"}
        onCambia={(acceso) => scegli(acceso ? "locali" : "spenta")}
      />
      <Interruttore
        etichetta={t("settings.roots.watch.network")}
        acceso={modo === "tutte"}
        onCambia={(acceso) => scegli(acceso ? "tutte" : "locali")}
        impedito={
          modo === "spenta" ? t("settings.roots.watch.network.off") : undefined
        }
      />
      {modo !== "spenta" && (quadro?.cartelle.length ?? 0) > 0 && (
        <p className="nota">
          {t("settings.roots.watch.state", {
            avvisi: quante("avvisi"),
            sondaggio: quante("sondaggio"),
          })}
        </p>
      )}
      {modo !== "spenta" && ferme > 0 && (
        <p className="nota">
          {t("settings.roots.watch.unreachable", { n: ferme })}
        </p>
      )}
      {diRete > 0 && (
        <p className="nota">
          {t("settings.roots.watch.stopped", { n: diRete })}
        </p>
      )}
      {quadro?.ultima && (
        <p className="nota">
          {t("settings.roots.watch.last", {
            quando: dataOra(quadro.ultima.quandoMs),
            inseriti: quadro.ultima.inseriti,
            aggiornati: quadro.ultima.aggiornati,
            tolti: quadro.ultima.tolti,
          })}
          {quadro.ultima.rinviate > 0 &&
            ` ${t("settings.roots.watch.held", { n: quadro.ultima.rinviate })}`}
        </p>
      )}
    </>
  );
}

/**
 * La scheda dei testi: l'interruttore della rete e la copertura.
 *
 * # Perché si gestisce da sé
 *
 * Perché `Impostazioni` riceve già sessantasette prop, e quattro in più per una
 * scheda che nessun'altra parte del programma guarda sarebbero quattro in più
 * da far scendere attraverso `App` a ogni ridisegno. Quel che serve a questa
 * scheda lo chiede lei quando viene montata, cioè quando qualcuno apre le
 * Impostazioni, e lo lascia andare quando si smonta.
 *
 * Lo stesso vale per l'avanzamento: arriva per evento, e l'evento lo ascolta
 * lei. Passarlo da `App` vorrebbe dire ridisegnare l'intera schermata delle
 * Impostazioni una volta per brano guardato.
 */
function SchedaTesti() {
  const [stato, setStato] = useState<StatoTesti | null>(null);

  /**
   * La scheda è ancora montata.
   *
   * Serve alla lettura e non agli ascolti: quelli li chiude `useAscolto`,
   * mentre `ipc.testiStato()` è una promessa in volo che può tornare dopo che
   * qualcuno ha già chiuso le Impostazioni. Si rimette a `true` al montaggio
   * perché in `StrictMode` il primo montaggio viene disfatto e rifatto, e un
   * riferimento che ricordasse solo lo smontaggio resterebbe spento per sempre.
   */
  const montata = useRef(true);
  useEffect(() => {
    montata.current = true;
    return () => {
      montata.current = false;
    };
  }, []);

  const aggiorna = useCallback(() => {
    ipc
      .testiStato()
      .then((letto) => {
        if (montata.current) setStato(letto);
      })
      .catch(() => {
        /* Una scheda che non sa dire i suoi numeri li lascia a zero: qui non
           c'è niente di distruttivo da annunciare, e un errore rosso nelle
           Impostazioni per una conta fallita sarebbe rumore. */
      });
  }, []);

  useEffect(() => {
    aggiorna();
  }, [aggiorna]);

  useAscolto<AvanzamentoTesti>("testi:avanzamento", (passo) => {
    setStato((prima) =>
      prima === null ? prima : { ...prima, inCorso: true, ...passo },
    );
  });

  useAscolto("testi:finito", aggiorna);

  // Una passata caduta a metà — la rete che se ne va, il servizio che dice di
  // no — non manda «finito», e senza questa riga la barra resterebbe a «in
  // corso» per sempre. È la stessa disciplina dell'arricchimento in `App.tsx`:
  // lo stato riletto dal nucleo è l'ultima parola su «sta girando», e
  // rileggerlo è quel che serve. Il guasto in sé non si mostra qui — chi ha
  // chiesto i testi non ha chiesto una finestra rossa — ma il numero dei
  // coperti torna vero, che è quello che si stava guardando.
  useAscolto("testi:guasto", aggiorna);

  const copertura = stato?.copertura ?? {
    sincronizzati: 0,
    piatti: 0,
    strumentali: 0,
    mancanti: 0,
  };

  return (
    <Scheda
      icona="i-text"
      titolo={t("settings.lyrics.title")}
      guida="lyrics"
    >

      <Interruttore
        etichetta={t("settings.lyrics.toggle")}
        acceso={stato?.rete ?? false}
        onCambia={(attivo) => {
          ipc
            .testiRete(attivo)
            .then(setStato)
            .catch(() => {});
        }}
      />

      {/* Spento finché la lettura non torna, com'è di serie: un interruttore
          dipinto acceso anche per un istante prometterebbe file nelle cartelle
          della musica che nessuno scrive. */}
      <Interruttore
        etichetta={t("settings.lyrics.sidecar")}
        acceso={stato?.accanto ?? false}
        onCambia={(attivo) => {
          ipc
            .testiAccanto(attivo)
            .then(setStato)
            .catch(() => {});
        }}
      />

      <dl className="numeri">
        <div>
          <dt>{t("settings.lyrics.synced")}</dt>
          <dd className="stat-number">{numero(copertura.sincronizzati)}</dd>
        </div>
        <div>
          <dt>{t("settings.lyrics.plain")}</dt>
          <dd className="stat-number">{numero(copertura.piatti)}</dd>
        </div>
        <div>
          <dt>{t("settings.lyrics.instrumental")}</dt>
          <dd className="stat-number">{numero(copertura.strumentali)}</dd>
        </div>
        <div>
          <dt>{t("settings.lyrics.missing")}</dt>
          <dd className="stat-number">{numero(copertura.mancanti)}</dd>
        </div>
      </dl>

      {stato?.inCorso && (
        <p className="esito">
          {t("settings.lyrics.progress", {
            fatti: stato.fatti,
            rimasti: stato.rimasti,
          })}
        </p>
      )}

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          /* Senza rete non c'è niente da chiedere, e senza domande aperte non
             c'è niente da cercare: un pulsante acceso prometterebbe qualcosa
             che non succede.

             Il conto è `inCoda` e non i mancanti qui accanto: sono due domande
             diverse — «di quanti non ho niente» e «quanti ne chiederei adesso»
             — e il pulsante appeso alla prima si spegneva su una libreria piena
             di testi piatti da sincronizzare, e si accendeva su una dove tutto
             quel che manca è già stato chiesto da poco. */
          disabled={
            stato === null ||
            !stato.rete ||
            (!stato.inCorso && stato.inCoda === 0)
          }
          onClick={() => {
            const chiamata = stato?.inCorso ? ipc.testiFerma() : ipc.testiRiempi();
            chiamata
              .then((letto) => {
                if (letto) setStato(letto);
              })
              .catch(() => {});
          }}
        >
          <Icona nome={stato?.inCorso ? "i-x" : "i-scan"} dim={15} />
          {stato?.inCorso
            ? t("settings.lyrics.stop")
            : t("settings.lyrics.fill")}
        </button>
      </div>

    </Scheda>
  );
}

/**
 * L'interruttore dei dati tecnici del file, sotto i comandi di riproduzione.
 *
 * # Perché si gestisce da sé
 *
 * La stessa ragione di `SchedaSpettro`, `SchedaUscita` e `SchedaChiusura`:
 * `Impostazioni` riceve già una sessantina di prop, e questa preferenza non la
 * guarda nessun'altra parte di questa schermata. Quel che gli serve se lo chiede
 * quando viene montato, e lo lascia andare quando si smonta.
 *
 * # Perché parte da acceso mentre quello dello spettro parte da spento
 *
 * Perché è il valore di serie della chiave, e questo è il valore che si mostra
 * finché la lettura non è tornata. Sono i due millisecondi in cui un
 * interruttore dipinto al contrario si vedrebbe lampeggiare.
 *
 * # Perché è pessimistico
 *
 * Perché lo stato arriva dal **ritorno** del comando e non dal click: il nucleo
 * scrive, rilegge e risponde con quel che c'è nel database. Una scrittura
 * fallita lascia l'interruttore dov'era invece di mostrarlo cambiato — e la
 * disciplina è quella di `SchedaChiusura` e dell'equalizzatore.
 */
function InterruttoreFormato({
  onErrore,
}: {
  /** La scelta scrive nel database: se non ci arriva, va detto. */
  onErrore: (e: unknown) => void;
}) {
  const [visibile, setVisibile] = useState(true);

  useEffect(() => {
    let annullato = false;
    ipc
      .formatoVisibile()
      .then((letto) => {
        if (!annullato) setVisibile(letto);
      })
      .catch(() => {
        /* Accesa, che è il valore di serie: una preferenza che non si legge non
           deve poter far sparire una riga che c'era. */
      });
    return () => {
      annullato = true;
    };
  }, []);

  return (
    <Interruttore
      etichetta={t("settings.fileFormat.toggle")}
      acceso={visibile}
      onCambia={(valore) => {
        ipc.formatoVisibileScegli(valore).then(setVisibile).catch(onErrore);
      }}
    />
  );
}

/** Ogni quanto si rilegge l'avanzamento della misura, finché la scheda è aperta. */
const AVANZAMENTO_VOLUME_MS = 15_000;

/**
 * Di chi è il livello del volume uniforme, e quanto della libreria è misurato.
 *
 * # Perché si gestisce da sé
 *
 * Come `InterruttoreFormato`: nessun'altra parte della schermata guarda il
 * modo, e l'avanzamento cambia da solo mentre la passata di sottofondo misura.
 * Se li chiede quando viene montato, e l'avanzamento lo richiede ogni
 * `AVANZAMENTO_VOLUME_MS` finché resta aperto.
 *
 * # Perché l'avanzamento si mostra
 *
 * Perché i brani senza tag si normalizzano man mano che vengono misurati, e la
 * passata gira solo quando la musica tace. Senza un numero, chi accende il
 * volume uniforme e sente ancora un disco più forte degli altri penserebbe che
 * non funzioni; con il numero sa che ci sta arrivando.
 *
 * Pessimistico come l'interruttore del formato: la linguetta si sposta al
 * ritorno del comando, con quel che il nucleo ha scritto davvero.
 */
function VolumeUniforme({
  onErrore,
}: {
  /** La scelta scrive nel database: se non ci arriva, va detto. */
  onErrore: (e: unknown) => void;
}) {
  const [modo, setModo] = useState<ModoLivello>("automatico");
  const [avanzamento, setAvanzamento] = useState<AvanzamentoVolume | null>(
    null,
  );

  useEffect(() => {
    let annullato = false;
    ipc
      .normalizzazioneModo()
      .then((letto) => {
        if (!annullato) setModo(letto);
      })
      .catch(() => {
        /* Automatico, che è il valore di serie. */
      });
    const leggi = () => {
      ipc
        .normalizzazioneAvanzamento()
        .then((letto) => {
          if (!annullato) setAvanzamento(letto);
        })
        .catch(() => {
          /* Senza numero non si mostra la riga: meglio niente che uno zero
             che non è vero. */
        });
    };
    leggi();
    const battito = window.setInterval(leggi, AVANZAMENTO_VOLUME_MS);
    return () => {
      annullato = true;
      window.clearInterval(battito);
    };
  }, []);

  return (
    <>
      <Segmentato
        etichetta={t("settings.loudness.mode.label")}
        scelta={modo}
        onScegli={(scelto) => {
          ipc.normalizzazioneModoScegli(scelto).then(setModo).catch(onErrore);
        }}
        voci={[
          { chiave: "automatico", etichetta: t("settings.loudness.mode.auto") },
          { chiave: "album", etichetta: t("settings.loudness.mode.album") },
          { chiave: "brano", etichetta: t("settings.loudness.mode.track") },
        ]}
      />
      {avanzamento !== null && avanzamento.totale > 0 && (
        <p className="nota">
          {avanzamento.misurati >= avanzamento.totale
            ? t("settings.loudness.done", { n: avanzamento.totale })
            : t("settings.loudness.progress", {
                fatti: avanzamento.misurati,
                totale: avanzamento.totale,
              })}
        </p>
      )}
    </>
  );
}

/**
 * Il cursore della dissolvenza incrociata.
 *
 * # Perché un cursore e non delle linguette
 *
 * Perché fra due e tre secondi c'è una differenza che si sente, e le linguette
 * costringerebbero a sceglierne quattro o cinque fra tredici valori. Un
 * cursore le ha tutte, e il numero accanto dice sempre a quale si è.
 *
 * # Perché la scelta vive qui mentre si trascina
 *
 * Per la ragione di `parti/Scrubber.tsx`: `onChange` scatta a ogni pixel, e un
 * `invoke` per pixel vorrebbe dire duecento scritture nel database per una
 * trascinata sola — su un percorso che prende anche il lucchetto del lettore.
 * Quindi il valore trascinato sta qui e si manda **al rilascio**: puntatore,
 * tastiera, o fuoco perso, che sono i tre modi in cui un cursore si lascia.
 */
function SchedaDissolvenza({
  dissolvenzaS,
  onDissolvenza,
}: {
  dissolvenzaS: number;
  onDissolvenza: (secondi: number) => void;
}) {
  const [trascinato, setTrascinato] = useState<number | null>(null);
  const dove = trascinato ?? dissolvenzaS;

  const rilascia = () => {
    if (trascinato === null) return;
    // Solo se è davvero cambiato: lasciare il cursore dov'era è un gesto
    // frequentissimo, e non deve costare una scrittura.
    if (trascinato !== dissolvenzaS) onDissolvenza(trascinato);
    setTrascinato(null);
  };

  return (
    <Scheda icona="i-play" titolo={t("settings.crossfade.title")} guida="crossfade">
      <div className="dissolvenza">
        <input
          type="range"
          className="scorrimento range-accent"
          min={0}
          max={DISSOLVENZA_MASSIMA_S}
          step={1}
          value={dove}
          style={
            {
              "--avanzamento": `${(dove / DISSOLVENZA_MASSIMA_S) * 100}%`,
            } as CSSProperties
          }
          aria-label={t("settings.crossfade.title")}
          aria-valuetext={
            dove === 0
              ? t("settings.crossfade.off")
              : t("settings.crossfade.seconds", { n: dove })
          }
          onChange={(e) => setTrascinato(Number(e.target.value))}
          onPointerUp={rilascia}
          onKeyUp={rilascia}
          onBlur={rilascia}
        />
        <span className="valore">
          {dove === 0
            ? t("settings.crossfade.off")
            : t("settings.crossfade.seconds", { n: dove })}
        </span>
      </div>
      {/* Detto solo quando è acceso: a zero il gapless c'è e la frase
          spiegherebbe la perdita di una cosa che non si sta perdendo. */}
      {dove > 0 && <p className="nota">{t("settings.crossfade.gapless")}</p>}
    </Scheda>
  );
}

/** Le durate fra cui si sceglie, in minuti. */
const DURATE_SPEGNIMENTO = [15, 30, 60] as const;

/**
 * Il timer di spegnimento.
 *
 * # Perché la scelta vive qui e non nel nucleo
 *
 * Perché quel che il nucleo sa è **quanto manca**, e quanto manca cala: dopo
 * cinque minuti un timer da mezz'ora vale venticinque, che non è nessuna delle
 * durate fra cui si è scelto. Derivare la linguetta accesa da quel numero
 * vorrebbe dire vederla scivolare da sola da «30» a «15» mentre la musica
 * suona.
 *
 * Quindi la linguetta mostra la **scelta**, che è di qui, e il tempo che manca
 * si legge sotto in lettere. Le due cose si riallineano da sole quando il
 * nucleo dice che il timer non c'è più — perché è scaduto, o perché l'ha
 * spento qualcun altro.
 */
function SchedaSpegnimento({
  spegnimentoMs,
  onSpegnimento,
}: {
  spegnimentoMs: number | null;
  onSpegnimento: (minuti: number) => void;
}) {
  const [scelta, setScelta] = useState<string>("spento");

  // Il nucleo è la verità: se il timer non c'è più, la linguetta torna a
  // «spento» qualunque cosa avesse scelto questa finestra.
  useEffect(() => {
    if (spegnimentoMs === null) setScelta("spento");
  }, [spegnimentoMs]);

  const voci = [
    { chiave: "spento", etichetta: t("settings.sleep.off") },
    ...DURATE_SPEGNIMENTO.map((minuti) => ({
      chiave: String(minuti),
      etichetta: t("settings.sleep.minutes", { n: minuti }),
    })),
    { chiave: "brano", etichetta: t("settings.sleep.endOfTrack") },
  ];

  return (
    <Scheda icona="i-play" titolo={t("settings.sleep.title")} guida="sleep">
      <Segmentato
        etichetta={t("settings.sleep.title")}
        scelta={scelta}
        onScegli={(chiave) => {
          setScelta(chiave);
          if (chiave === "spento") onSpegnimento(0);
          else if (chiave === "brano") onSpegnimento(FINE_DEL_BRANO);
          else onSpegnimento(Number(chiave));
        }}
        voci={voci}
      />
      {spegnimentoMs !== null && (
        <p className="nota">
          {spegnimentoMs === 0
            ? t("settings.sleep.atEnd")
            : t("settings.sleep.left", { tempo: durata(spegnimentoMs) })}
        </p>
      )}
    </Scheda>
  );
}

/**
 * La scheda della chiusura: cosa fa la X.
 *
 * # Perché si gestisce da sé
 *
 * La stessa ragione di `SchedaTesti`: `Impostazioni` riceve già sessantasette
 * prop, e questa preferenza non la guarda nessun'altra parte del programma —
 * la legge il gestore della chiusura, che sta in Rust e non passa di qui.
 * Passarla da `App` vorrebbe dire due prop in più lungo tutta la catena per un
 * booleano che vive dentro una scheda sola.
 *
 * # Perché l'interruttore è pessimistico
 *
 * Perché lo stato arriva dal **ritorno** del comando e non dal click. È la
 * disciplina di `accentoDinamicoAttiva`, e non quella del tema o della lingua,
 * che si accendono subito. Quelle si vedono succedere: la finestra cambia
 * colore, e una scrittura fallita si nota perché il colore non è cambiato. Qui
 * non c'è niente da vedere — l'effetto è quel che farà la X fra tre ore — e un
 * interruttore acceso su una scrittura fallita mentirebbe fino alla prima volta
 * che qualcuno chiude Aether credendo di nasconderlo.
 */
function SchedaChiusura({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [acceso, setAcceso] = useState(false);

  useEffect(() => {
    let annullato = false;
    ipc
      .secondoPiano()
      .then((letto) => {
        if (!annullato) setAcceso(letto);
      })
      .catch(() => {
        /* Spento, che è il valore di serie e il comportamento di sempre: una
           preferenza che non si legge non deve poter far sparire la finestra. */
      });
    return () => {
      annullato = true;
    };
  }, []);

  return (
    <Scheda
      icona="i-x"
      titolo={t("settings.background.title")}
      guida="background"
    >
      <Interruttore
        etichetta={t("settings.background.toggle")}
        acceso={acceso}
        onCambia={(valore) => {
          ipc.secondoPianoAttiva(valore).then(setAcceso).catch(onErrore);
        }}
      />
    </Scheda>
  );
}

/** Da byte a una misura che si legge: nessun decimale sotto il megabyte. */
function peso(byte: number): string {
  const mb = byte / (1024 * 1024);
  if (mb < 1) {
    return t("settings.maintenance.kb", { n: Math.round(byte / 1024) });
  }
  // `numero` e non `toFixed`: la virgola decimale è della lingua, non di JavaScript.
  if (mb < 1024) return t("settings.maintenance.mb", { n: numero(mb, 1) });
  return t("settings.maintenance.gb", { n: numero(mb / 1024, 2) });
}

/** La riga sulla salute del database, per ognuno dei sei esiti. */
function fraseSalute(salute: SaluteDatabase): string {
  switch (salute.esito) {
    case "daFare":
      return t("settings.maintenance.health.pending");
    case "sano":
      return t("settings.maintenance.health.ok", {
        quando: quando(salute.quandoMs),
      });
    case "interrotto":
      return t("settings.maintenance.health.interrupted");
    case "saltato":
      return t("settings.maintenance.health.skipped");
    // `db.corrupt` ha già la sua frase, e dice cosa fare: ripristinare un
    // backup o riscansionare. Riscriverla qui ne farebbe due da tenere allineate.
    case "danneggiato":
      return testoErrore(salute.errore);
    case "nonRiuscito":
      return t("settings.maintenance.health.failed", {
        errore: testoErrore(salute.errore),
      });
  }
}

/**
 * La cache delle copertine e la salute del database.
 *
 * # Perché si gestisce da sé
 *
 * La stessa ragione di `SchedaTesti` e `SchedaChiusura`: nessun'altra parte
 * del programma guarda questi numeri, e farli scendere da `App` vorrebbe dire
 * tre prop in più lungo tutta la catena. Li chiede lei quando qualcuno apre la
 * sezione, e li lascia andare quando si chiude.
 *
 * # Perché il pulsante non chiede conferma
 *
 * Perché quel che toglie non si perde: sono copertine che nessuna riga del
 * database nomina più, e se un brano che le aveva torna in libreria la
 * scansione le rilegge dal suo file. La pulizia gira comunque da sola dopo ogni
 * scansione; il pulsante è per chi vuole lo spazio adesso.
 */
function SchedaManutenzione({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [stato, setStato] = useState<StatoManutenzione | null>(null);
  const [guasto, setGuasto] = useState<unknown>(null);
  const [pulendo, setPulendo] = useState(false);
  const [liberati, setLiberati] = useState<{ byte: number; file: number } | null>(
    null,
  );

  useEffect(() => {
    let annullato = false;
    ipc
      .manutenzioneStato()
      .then((letto) => {
        if (!annullato) setStato(letto);
      })
      .catch((e: unknown) => {
        // Qui e non in `onErrore`: una misura che non si legge non merita la
        // fascia rossa in cima alla pagina, merita una riga dentro la scheda.
        if (!annullato) setGuasto(e);
      });
    return () => {
      annullato = true;
    };
  }, []);

  // Il controllo gira un minuto dopo l'avvio: chi ha le Impostazioni aperte
  // in quel momento deve vederne l'esito senza richiuderle.
  useAscolto<SaluteDatabase>("manutenzione:salute", (salute) => {
    setStato((prima) => (prima === null ? prima : { ...prima, salute }));
  });

  const pulisci = () => {
    setPulendo(true);
    setLiberati(null);
    ipc
      .manutenzionePulisciCopertine()
      .then((esito) => {
        setStato(esito.stato);
        setLiberati({ byte: esito.byte, file: esito.file });
      })
      .catch(onErrore)
      .finally(() => setPulendo(false));
  };

  const danneggiato = stato?.salute.esito === "danneggiato";

  return (
    <Scheda
      icona="i-album"
      titolo={t("settings.maintenance.title")}
      guida="maintenance"
    >
      {guasto !== null && <p className="errore">{testoErrore(guasto)}</p>}
      {stato && (
        <>
          <p className="nota">
            {t("settings.maintenance.cache", {
              peso: peso(stato.byte),
              n: stato.file,
            })}{" "}
            {stato.orfane > 0 &&
              t("settings.maintenance.orphans", { n: numero(stato.orfane) })}
          </p>
          {danneggiato ? (
            <p className="errore">{fraseSalute(stato.salute)}</p>
          ) : (
            <p className="nota">{fraseSalute(stato.salute)}</p>
          )}
        </>
      )}
      {liberati && (
        <p className="esito">
          {liberati.file === 0
            ? t("settings.maintenance.nothing")
            : t("settings.maintenance.cleaned", {
                peso: peso(liberati.byte),
                n: liberati.file,
              })}
        </p>
      )}
      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          /* Su un database danneggiato non si scrive: il nucleo rifiuterebbe
             comunque, e un pulsante acceso prometterebbe un gesto che non
             succede. */
          disabled={pulendo || stato === null || danneggiato}
          onClick={pulisci}
        >
          {pulendo
            ? t("settings.maintenance.cleaning")
            : t("settings.maintenance.clean")}
        </button>
      </div>
    </Scheda>
  );
}

export function Impostazioni({
  sezione,
  onSezione,
  avvio,
  scansione,
  esito,
  skin,
  ripiego,
  accentoPermesso,
  accentoDinamico,
  onAccentoDinamico,
  movimento,
  movimentoUtente,
  onMovimentoUtente,
  zoom,
  onZoom,
  onZoomNormale,
  tema,
  onTema,
  lingua,
  onLingua,
  scorciatoie,
  onScorciatoie,
  onGiro,
  onProfiloImportato,
  eqAttivo,
  eqGuadagni,
  replaygain,
  onReplaygain,
  spegnimentoMs,
  onSpegnimento,
  autoplay,
  onAutoplay,
  dissolvenzaS,
  onDissolvenza,
  onErrore,
  onNotizia,
  onAggiungiCartella,
  onTogliCartella,
  onScegliCartellaDownload,
  onCartellaDownloadDiSerie,
  onScansiona,
  onAnnullaScansione,
  onScegliSkin,
  onAnteprimaSkin,
  onInstallaSkin,
  onCreaTema,
  onDeriva,
  onDisinstallaSkin,
  onApriStudio,
  onImporta,
  onImportaLink,
  onImportaAccount,
  onVista,
  importazioni,
  arricchimento,
  avanzaArricchimento,
  esitoArricchimento,
  onArricchimentoAttiva,
  onArricchimentoAnnulla,
  onArricchimentoRiportaNeiFile,
  nuvola,
  onNuvolaCollega,
  onNuvolaScollega,
  onNuvolaAttiva,
  onNuvolaSalva,
  onNuvolaRipristina,
  onNuvolaCredenziali,
  sincronia,
  onSincroniaAttiva,
  onSincroniaAdesso,
  onSincroniaMagazzino,
  onSincroniaAccoppia,
  onSincroniaDimentica,
}: {
  sezione: Sezione;
  onSezione: (s: Sezione) => void;
  avvio: Avvio | null;
  scansione: Avanzamento | null;
  esito: EsitoScansione | null;
  skin: VoceSkin[];
  /**
   * La skin scelta che non si è potuta leggere, e per cui si indossa quella di
   * serie. `null` quando quel che si vede è quel che si è scelto.
   */
  ripiego: string | null;
  /**
   * La skin attiva permette l'accento dinamico (`capabilities.dynamicAccent`).
   *
   * È una dichiarazione dell'autore e vince sulla preferenza: una skin può
   * essere costruita attorno al suo accento, e `sala` lo è.
   */
  accentoPermesso: boolean;
  /** La preferenza, com'è nel database. */
  accentoDinamico: boolean;
  onAccentoDinamico: (attivo: boolean) => void;
  /** Il movimento che la skin dichiara: `none`, `essential`, `full`, `maximum`. */
  movimento: string;
  /** Quanto movimento vuole chi guarda, **sotto** quello della skin. */
  movimentoUtente: MovimentoUtente;
  /**
   * La scrive `useAspetto` (`aspettoFinestra.ts`), che è chi la fa valere sul
   * documento.
   */
  onMovimentoUtente: (scelta: MovimentoUtente) => void;
  /**
   * Di quanto è ingrandita l'interfaccia: 1 è la misura di serie.
   *
   * Un numero e non un `number | null`, al contrario dello stato che lo tiene
   * in `aspettoFinestra.ts`: quando questa schermata si apre lo zoom è
   * arrivato da un pezzo — la finestra non si mostra prima — e il caso che
   * resta è quello in cui la lettura è fallita, dove la misura vera è davvero
   * quella di serie.
   * Farne un `null` di qua vorrebbe dire un secondo stato «non si sa ancora»
   * in una schermata che non può vederlo.
   */
  zoom: number;
  /**
   * Un gradino in su (`true`) o in giù (`false`).
   *
   * Un verso e non un numero, come per la scorciatoia: la scala dei gradini
   * vive in `preferenze::SCALA_ZOOM` e non attraversa l'IPC. Questa schermata
   * non sa quali misure esistono, e non deve saperlo — sa che ce n'è una
   * prima e una dopo, e che ai due capi non succede niente.
   */
  onZoom: (su: boolean) => void;
  /** Torna alla misura di serie. */
  onZoomNormale: () => void;
  tema: Tema;
  onTema: (t: Tema) => void;
  /** Il codice della lingua in uso. */
  lingua: string;
  /** La scrive `useAspetto` (`aspettoFinestra.ts`), che la fa valere. */
  onLingua: (codice: string) => void;
  /** Le associazioni fra tasti e comandi, come sono in uso adesso. */
  scorciatoie: Associazioni;
  /** Le nuove, già intere: le scrive `useAspetto`, che è chi le fa valere. */
  onScorciatoie: (a: Associazioni) => void;
  /**
   * Rifà il giro guidato, subito.
   *
   * Una prop e non una chiamata da qui, per la stessa ragione per cui il giro
   * non vive in questa schermata: i dieci riflettori puntano su comandi che
   * stanno **fuori** dalle Impostazioni, e ad aprirli e chiuderli è `App`.
   */
  onGiro: () => void;
  /** Un profilo è stato applicato: quel che sta in `App` va riletto. */
  onProfiloImportato: () => void;
  /** L'equalizzatore è acceso, secondo il nucleo. */
  eqAttivo: boolean;
  /** La sua curva, in decibel per banda. */
  eqGuadagni: number[];
  /** A che livello normalizza il volume, secondo il nucleo. */
  replaygain: Normalizzazione;
  onReplaygain: (livello: Normalizzazione) => void;
  /** Fra quanto si spegne da solo: `null` nessun timer, `0` a fine brano. */
  spegnimentoMs: number | null;
  onSpegnimento: (minuti: number) => void;
  /** A coda finita si continua da soli. */
  autoplay: boolean;
  onAutoplay: (attivo: boolean) => void;
  /** Quanto si sovrappongono due brani, in secondi. `0` è spenta. */
  dissolvenzaS: number;
  onDissolvenza: (secondi: number) => void;
  onErrore: (e: unknown) => void;
  /** Una cosa andata bene da dire: non è un errore e non va sul canale rosso. */
  onNotizia: (testo: string) => void;
  onAggiungiCartella: () => void;
  onTogliCartella: (percorso: string) => void;
  /** Apre il dialogo che sceglie dove finiscono i brani scaricati. */
  onScegliCartellaDownload: () => void;
  /** Rimette il valore di serie: la prima cartella sorvegliata. */
  onCartellaDownloadDiSerie: () => void;
  onScansiona: () => void;
  onAnnullaScansione: () => void;
  onScegliSkin: (id: string) => void;
  /** Anteprima al passaggio: compila senza scegliere. `null` la revoca. */
  onAnteprimaSkin: (id: string | null) => void;
  onInstallaSkin: () => void;
  /** Apre la finestrella che battezza un tema nuovo. */
  onCreaTema: () => void;
  /** Come `onCreaTema`, con questa skin già scelta come base. */
  onDeriva: (id: string) => void;
  /** Toglie una skin installata. Quella di serie non passa di qui. */
  onDisinstallaSkin: (id: string) => void;
  /** Apre lo Skin Studio su questa skin. */
  onApriStudio: (id: string) => void;
  onImporta: () => void;
  /**
   * Apre la finestrella in cui si incolla un link.
   *
   * Il servizio è un **suggerimento**, non un filtro: dice alla finestrella
   * quale segnaposto mostrare e di quale dei due servizi diagnosticare i
   * guasti. Un link dell'altro continua a funzionare — a riconoscerlo è il
   * nucleo, e obbligare a dichiararlo prima vorrebbe dire far scegliere una
   * cosa che si sa solo dopo aver guardato negli appunti.
   */
  onImportaLink: () => void;
  /**
   * Apre la finestrella dell'account intero.
   *
   * Un secondo callback e non un parametro del primo: sono due operazioni
   * diverse — una legge un link pubblico, l'altra porta dentro un account —
   * e un booleano che sceglie fra due schermate è la firma che poi nessuno
   * ricorda in che verso va.
   */
  onImportaAccount: () => void;
  /**
   * Porta a una pagina che non è una sezione di qui.
   *
   * Serve al rimando della coda. Un `onVista` e non un `onImportazioni`: la
   * schermata non deve sapere **quale** pagina è, deve sapere che si esce da
   * qui — e la prossima uscita non aggiunge una prop.
   */
  onVista: (v: Vista) => void;
  /**
   * Le importazioni da un servizio esterno e la coda che le scarica.
   *
   * Un oggetto solo e non otto prop sciolte: è uno stato coeso che vive in
   * `FornitoreImportazioni` — la coda gira anche quando questa schermata non è
   * aperta — e passa di qui soltanto per essere mostrato. Qui ne resta il
   * **conteggio**: il resto sta nella pagina.
   */
  importazioni: UsoImportazioni;
  /** Lo stato dell'arricchimento, o `null` finché non è stato chiesto. */
  arricchimento: StatoArricchimento | null;
  /** A che punto è la passata in corso, in gruppi d'album. `null` = ferma. */
  avanzaArricchimento: AvanzamentoArricchimento | null;
  /** Cosa ha prodotto l'ultima passata di questa sessione. */
  esitoArricchimento: EsitoArricchimento | null;
  onArricchimentoAttiva: (attivo: boolean) => void;
  /** Dimentica quel che l'arricchimento ha dedotto, e spegne l'automatico. */
  onArricchimentoAnnulla: () => void;
  /**
   * Riscrive nei file i tag di prima, per quel che le versioni passate hanno
   * già toccato.
   *
   * Una prop a parte e non un parametro della precedente: sono due gesti
   * diversi — uno svuota una tabella, l'altro riapre in scrittura migliaia di
   * file dell'utente — e un booleano che sceglie fra i due sarebbe la firma
   * che poi nessuno ricorda in che verso va.
   */
  onArricchimentoRiportaNeiFile: () => void;
  /** Lo stato del backup, o `null` finché non è stato chiesto. */
  nuvola: StatoNuvola | null;
  onNuvolaCollega: () => void;
  onNuvolaScollega: () => void;
  onNuvolaAttiva: (attivo: boolean) => void;
  onNuvolaSalva: () => void;
  /** Apre la finestrella del ripristino. */
  onNuvolaRipristina: () => void;
  onNuvolaCredenziali: (clientId: string, clientSecret: string) => void;
  /** Lo stato della sincronia, o `null` finché non è stato chiesto. */
  sincronia: StatoSincronia | null;
  onSincroniaAttiva: (accesa: boolean) => void;
  onSincroniaAdesso: () => void;
  onSincroniaMagazzino: (
    dove: "cartella" | "drive",
    cartella: string | null,
  ) => void;
  onSincroniaAccoppia: (id: string, nome: string | null) => void;
  onSincroniaDimentica: (id: string) => void;
}) {
  const [clientId, setClientId] = useState("");
  /**
   * Il percorso che si sta scrivendo, prima di confermarlo.
   *
   * `null` vuol dire «non lo sto scrivendo»: il campo mostra allora quello
   * salvato. Senza questa distinzione, ogni evento `sincronia:stato` che
   * arriva mentre si scrive riporterebbe il campo al valore di prima —
   * cancellando quel che si stava digitando.
   */
  const [cartellaSincronia, setCartellaSincronia] = useState<string | null>(
    null,
  );
  /** I nomi che si stanno scrivendo per i dispositivi, prima di confermarli. */
  const [nomi, setNomi] = useState<Record<string, string>>({});
  const [clientSecret, setClientSecret] = useState("");
  /**
   * I passi per fabbricarsi le credenziali sono aperti.
   *
   * Chiusi all'apertura e non aperti: sono sei passi da fare **una volta**, su
   * un sito che non è questo, e chi ha già le sue credenziali in mano tornerà
   * qui per incollarle — non per rileggere come si ottengono.
   */
  const [guidaDrive, setGuidaDrive] = useState(false);
  /** Il filtro sopra l'indice. Tredici sezioni sono oltre il punto in cui si scorre. */
  const [filtro, setFiltro] = useState("");
  const trovate = cerca(filtro);
  const attiva = skin.find((s) => s.attiva);
  const avanzaNuvola = useAvanzamentoNuvola(nuvola?.inCorso ?? false);
  /** Quanti brani restano da scaricare: il numero dietro la porta del rimando. */
  const inCoda = importazioni.stato?.conteggi.attesa ?? 0;
  /** Dove i brani finiscono davvero: la scelta, o la prima sorvegliata. */
  const cartellaScarichi =
    avvio?.cartellaDownload ?? avvio?.cartelle[0] ?? null;
  /**
   * La cartella scelta non sta sotto nessuna di quelle sorvegliate.
   *
   * Il confronto è testuale e volutamente grezzo — normalizza le barre e
   * ignora le maiuscole, che è quanto serve su Windows — perché il caso da
   * prendere è quello grossolano: `D:\Scarichi` mentre si sorveglia
   * `C:\Musica`. Un giudizio più fine su collegamenti simbolici e percorsi UNC
   * lo può dare solo il nucleo, che guarda il disco; qui basta non stare zitti.
   */
  const fuoriDalleSorvegliate = (() => {
    if (cartellaScarichi === null) return false;
    const normale = (p: string) =>
      p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
    const scelta = normale(cartellaScarichi);
    return !(avvio?.cartelle ?? []).some((c) => {
      const radice = normale(c);
      return scelta === radice || scelta.startsWith(`${radice}/`);
    });
  })();
  const percentuale =
    scansione && scansione.totale > 0
      ? Math.round((scansione.fatti / scansione.totale) * 100)
      : 0;

  return (
    <ErroreGuida.Provider value={onErrore}>
      <div className="impostazioni">
        <nav className="indice settings-nav" aria-label={t("settings.index.aria")}>
          <div className="filtro-impostazioni">
            <Icona nome="i-search" dim={14} />
            <input
              type="search"
              value={filtro}
              placeholder={t("settings.index.search")}
              aria-label={t("settings.index.search")}
              onChange={(e) => setFiltro(e.target.value)}
              /* Invio va alla prima trovata: chi ha scritto «replaygain» e vede
                 una riga sola non deve staccare la mano dalla tastiera per
                 cliccarla. */
              onKeyDown={(e) => {
                const prima = trovate[0];
                if (e.key === "Enter" && prima) {
                  onSezione(prima.sezione);
                  setFiltro("");
                }
              }}
            />
          </div>

          {filtro.trim() === "" ? (
            gruppi().map(([titolo, dentro]) => (
              /* Un frammento e non un `<div>`: l'indice è una colonna sola, e un
                 contenitore per gruppo vorrebbe dire quattro scatole da far
                 comportare come se non ci fossero. */
              <Fragment key={titolo}>
                <div className="titolo-gruppo">{titolo}</div>
                {dentro.map(([chiave, etichetta, icona]) => (
                  <button
                    key={chiave}
                    type="button"
                    className="voce nav-pill"
                    aria-current={sezione === chiave ? "true" : undefined}
                    data-active={sezione === chiave || undefined}
                    onClick={() => onSezione(chiave)}
                  >
                    <Icona nome={icona} dim={16} />
                    <span>{etichetta}</span>
                  </button>
                ))}
              </Fragment>
            ))
          ) : trovate.length === 0 ? (
            <p className="niente empty-state">{t("settings.index.nothing")}</p>
          ) : (
            trovate.map((v) => {
              const dove = sezioni().find(([chiave]) => chiave === v.sezione);
              return (
                <button
                  key={`${v.sezione}/${v.titolo}`}
                  type="button"
                  className="voce nav-pill trovata"
                  onClick={() => {
                    onSezione(v.sezione);
                    // Il filtro si svuota: lasciarlo pieno vorrebbe dire tornare
                    // in una pagina il cui indice non mostra più dove si è.
                    setFiltro("");
                  }}
                >
                  <Icona nome={dove?.[2] ?? "i-search"} dim={16} />
                  <span>
                    {v.titolo}
                    <em className="dove">{dove?.[1] ?? ""}</em>
                  </span>
                </button>
              );
            })
          )}
        </nav>

        <div className="corpo">
          {sezione === "cartelle" && (
            <Scheda
              icona="i-folder"
              titolo={t("settings.roots.title")}
              guida="roots"
            >
              {avvio && avvio.cartelle.length > 0 ? (
                <ul className="cartelle">
                  {avvio.cartelle.map((c) => (
                    <li className="cartella" key={c}>
                      {/* `unicode-bidi: plaintext` accanto a `direction: rtl` sta
                          nel foglio: senza, un percorso UNC che comincia con due
                          barre rovesciate si disegnava con quelle in fondo. */}
                      <span className="percorso" title={c}>
                        {c}
                      </span>
                      <button
                        type="button"
                        className="tasto icon-btn"
                        aria-label={t("settings.roots.forget", { percorso: c })}
                        title={t("settings.roots.forget.title")}
                        onClick={() => onTogliCartella(c)}
                      >
                        <Icona nome="i-x" dim={14} />
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="niente empty-state">{t("settings.roots.empty")}</p>
              )}

              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={onAggiungiCartella}
                >
                  <Icona nome="i-plus" dim={15} />
                  {t("settings.roots.add")}
                </button>
                <button
                  type="button"
                  className="bottone primario btn-accent"
                  onClick={onScansiona}
                  disabled={
                    scansione !== null || (avvio?.cartelle.length ?? 0) === 0
                  }
                >
                  <Icona nome="i-scan" dim={15} />
                  {scansione
                    ? t("settings.roots.scanning")
                    : t("settings.roots.scan")}
                </button>
                <span className="oppure">{t("settings.roots.orDrop")}</span>
              </div>

              <RileggiTag
                occupato={
                  scansione !== null || (avvio?.cartelle.length ?? 0) === 0
                }
                onScansiona={onScansiona}
                onErrore={onErrore}
              />

              <SorveglianzaCartelle onErrore={onErrore} />

              {scansione && (
                <div className="avanzamento-scansione">
                  <div className="riga-avanzamento">
                    <span className="quanti">
                      {scansione.totale > 0
                        ? `${numero(scansione.fatti)} / ${numero(scansione.totale)}`
                        : t("settings.scan.comparing")}
                    </span>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      onClick={onAnnullaScansione}
                    >
                      {t("common.cancel")}
                    </button>
                  </div>
                  <div className="barra player-progress">
                    <div
                      className="riempita"
                      style={{ width: `${percentuale}%` }}
                    >
                      <span
                        className="riflesso progress-sheen"
                        aria-hidden="true"
                      />
                    </div>
                  </div>
                </div>
              )}

              {esito && (
                <p className="esito">
                  {t("settings.scan.result", {
                    inseriti: esito.inseriti,
                    aggiornati: esito.aggiornati,
                    spostati: esito.spostati,
                    tolti: esito.tolti,
                    secondi: numero(esito.durataMs / 1000, 1),
                  })}
                  {esito.illeggibili > 0 &&
                    t("settings.scan.unreadable", { n: esito.illeggibili })}
                  {/* Chi ha premuto Annulla deve leggere che si è fermata, non
                      «completata»: la libreria è giusta ma incompleta, e il modo
                      di finirla è rifarla. */}
                  {esito.annullata && (
                    <>
                      {" · "}
                      <strong>{t("settings.scan.stopped.strong")}</strong>
                      {t("settings.scan.stopped.rest")}
                    </>
                  )}
                  {/* Una radice che non ha risposto è tutta la differenza fra
                      «quei brani non ci sono più» e «quei brani non li ho
                      guardati». Senza questa riga la scansione dice
                      «completata», e chi la legge conclude la prima delle due
                      mentre è vera la seconda. I percorsi si scrivono per
                      intero: sapere che *una* cartella manca non serve a
                      niente se non si sa quale ricollegare. */}
                  {esito.radiciSaltate.length > 0 && (
                    <>
                      {" · "}
                      <strong>
                        {t("settings.scan.skipped.strong", {
                          n: esito.radiciSaltate.length,
                        })}
                      </strong>
                      {t("settings.scan.skipped.rest")}{" "}
                      {esito.radiciSaltate.join(" · ")}
                    </>
                  )}
                  {/* Le sottocartelle che non si sono lasciate leggere: i brani
                      lì sotto non sono stati né tolti né riletti, e l'elenco
                      dice dove guardare. */}
                  {esito.cartelleIlleggibili.length > 0 && (
                    <>
                      {" · "}
                      <strong>
                        {t("settings.scan.lostFolders.strong", {
                          n: esito.cartelleIlleggibili.length,
                        })}
                      </strong>
                      {t("settings.scan.lostFolders.rest")}{" "}
                      {esito.cartelleIlleggibili.join(" · ")}
                    </>
                  )}
                  {/* Dalla 3.0 anche la scansione a mano trattiene una sparizione
                      di massa e la chiede con `ConfermaRimozioni`; se la domanda
                      si chiude con «Tienili», la riga resta a ricordarlo. */}
                  {esito.rimozioniRinviate > 0 && (
                    <>
                      {" · "}
                      <strong>
                        {t("settings.scan.held.strong", {
                          n: esito.rimozioniRinviate,
                        })}
                      </strong>
                      {t("settings.scan.held.rest")}
                    </>
                  )}
                </p>
              )}
            </Scheda>
          )}

          {sezione === "cartelle" && (
            <Scheda
              icona="i-import"
              titolo={t("settings.downloads.title")}
              guida="downloads"
              nota={
                avvio?.cartellaDownload === null
                  ? t("settings.downloads.default")
                  : t("settings.downloads.chosen")
              }
            >

              {(avvio?.cartelle.length ?? 0) === 0 ? (
                <p className="niente empty-state">
                  {t("settings.downloads.needRoot")}
                </p>
              ) : (
                <>
                  <div className="cartella-scarichi">
                    <span
                      className="percorso"
                      title={cartellaScarichi ?? undefined}
                    >
                      {cartellaScarichi}
                    </span>
                  </div>
                  {fuoriDalleSorvegliate && (
                    <p className="avviso">{t("settings.downloads.outside")}</p>
                  )}
                  <div className="azioni">
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      onClick={onScegliCartellaDownload}
                    >
                      <Icona nome="i-folder" dim={15} />
                      {t("settings.downloads.change")}
                    </button>
                    {avvio?.cartellaDownload !== null && (
                      <button
                        type="button"
                        className="bottone btn-ghost"
                        onClick={onCartellaDownloadDiSerie}
                      >
                        <Icona nome="i-x" dim={15} />
                        {t("settings.downloads.reset")}
                      </button>
                    )}
                  </div>
                </>
              )}
            </Scheda>
          )}

          {/* Accanto alle cartelle e non in una sezione sua: l'arricchimento è
              quel che succede ai file **dopo** che la scansione li ha trovati, e
              chi viene qui a sistemare la libreria è chi ha bisogno di trovarlo. */}
          {sezione === "cartelle" && (
            <Scheda
              icona="i-scan"
              titolo={t("settings.enrich.title")}
              guida="enrich"
            >

              {arricchimento?.errore && (
                <div className="errore">{testoErrore(arricchimento.errore)}</div>
              )}

              <Interruttore
                etichetta={t("settings.enrich.toggle")}
                acceso={arricchimento?.attivo ?? false}
                onCambia={onArricchimentoAttiva}
              />

              <dl className="numeri">
                <div>
                  <dt>{t("settings.enrich.done")}</dt>
                  <dd className="stat-number">
                    {numero(arricchimento?.completati ?? 0)}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.enrich.noMatch")}</dt>
                  <dd className="stat-number">
                    {numero(arricchimento?.senzaCorrispondenza ?? 0)}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.enrich.waiting")}</dt>
                  <dd className="stat-number">
                    {numero(arricchimento?.daFare ?? 0)}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.enrich.lastPass")}</dt>
                  <dd className="stat-number">
                    {arricchimento?.inCorso
                      ? t("settings.enrich.inProgress")
                      : quando(arricchimento?.ultimoMs ?? null)}
                  </dd>
                </div>
              </dl>

              {avanzaArricchimento && (
                <div className="avanzamento-scansione">
                  <div className="riga-avanzamento">
                    <span className="quanti">
                      {t("settings.enrich.records", {
                        fatti: avanzaArricchimento.fatti,
                        totale: avanzaArricchimento.totale,
                      })}
                    </span>
                  </div>
                  <div className="barra player-progress">
                    <div
                      className="riempita"
                      style={{
                        width: `${
                          avanzaArricchimento.totale > 0
                            ? Math.round(
                                (avanzaArricchimento.fatti /
                                  avanzaArricchimento.totale) *
                                  100,
                              )
                            : 0
                        }%`,
                      }}
                    >
                      <span
                        className="riflesso progress-sheen"
                        aria-hidden="true"
                      />
                    </div>
                  </div>
                </div>
              )}

              {esitoArricchimento && !avanzaArricchimento && (
                <p className="esito">
                  {t("settings.enrich.result", {
                    applicati: esitoArricchimento.applicati,
                    astenuti: esitoArricchimento.astenuti,
                    senza: esitoArricchimento.senzaCorrispondenza,
                  })}
                  {esitoArricchimento.copertine > 0 &&
                    t("settings.enrich.result.covers", {
                      n: esitoArricchimento.copertine,
                    })}
                </p>
              )}

              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  /* Senza righe da riportare non c'è niente da disfare, e un
                     pulsante attivo prometterebbe qualcosa che non succede. */
                  disabled={
                    arricchimento === null ||
                    arricchimento.inCorso ||
                    arricchimento.annullabili === 0
                  }
                  onClick={onArricchimentoAnnulla}
                >
                  <Icona nome="i-x" dim={15} />
                  {arricchimento?.inCorso
                    ? t("settings.enrich.passRunning")
                    : arricchimento && arricchimento.annullabili > 0
                      ? t("settings.enrich.undo.count", {
                          n: arricchimento.annullabili,
                        })
                      : t("settings.enrich.undo")}
                </button>

                {/* Compare solo se c'è davvero qualcosa da riportare, e non
                    compare spento: un pulsante disabilitato dice «un giorno
                    potrai», e qui quel giorno non arriva mai — `neiFile` conta
                    le righe di `enrich_undo`, che dalla 2.3.1 nessuno scrive
                    più e che possono solo calare. Su una libreria nata da
                    questa versione in poi questo bottone non deve esistere
                    affatto. */}
                {arricchimento !== null && arricchimento.neiFile > 0 && (
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={arricchimento.inCorso}
                    onClick={onArricchimentoRiportaNeiFile}
                  >
                    <Icona nome="i-import" dim={15} />
                    {arricchimento.inCorso
                      ? t("settings.enrich.passRunning")
                      : t("settings.enrich.toFiles.count", {
                          n: arricchimento.neiFile,
                        })}
                  </button>
                )}
              </div>

              {/* Il salto che va spiegato, e che senza questa nota si legge come
                  un guasto: su una libreria che viene dalla 2.3.0 «Dimentica»
                  mostra **zero** finché non gira una passata nuova. È corretto —
                  quei brani hanno una riga in `enrich_undo` e non nella tabella
                  nuova, e i loro tag stanno dentro i file, non in una tabella di
                  Aether — ma nessuno può indovinarlo guardando un contatore a
                  zero accanto a una libreria che l'arricchimento ha
                  evidentemente toccato. La condizione è la stessa del bottone
                  qui sopra, ed è quella giusta: la nota c'è esattamente finché
                  esiste il caso che spiega, e sparisce da sé il giorno in cui la
                  tabella vecchia si svuota. */}
              {arricchimento !== null && arricchimento.neiFile > 0 && (
                <p className="nota">
                  <Trans
                    k="settings.enrich.legacy.note"
                    n={{ n: arricchimento.neiFile }}
                    v={{
                      riporta: <strong>{t("settings.enrich.toFiles")}</strong>,
                    }}
                  />
                </p>
              )}
            </Scheda>
          )}

          {/* Dopo l'arricchimento, e per la stessa ragione per cui
              l'arricchimento sta qui: sono tutt'e due quel che succede ai brani
              **dopo** che la scansione li ha trovati. */}
          {sezione === "cartelle" && <SchedaTesti />}

          {sezione === "aspetto" && (
            <>
              <Scheda icona="i-skin" titolo={t("settings.language.title")} guida="language">
                {/* L'elenco non è scritto qui: è quel che `import.meta.glob`
                    trova dentro `src/lingue/`. Un `de.json` lasciato cadere lì
                    compare in questo controllo senza che questa riga cambi, ed è
                    l'unico modo di rendere vera la promessa invece che dichiararla.
                    Sotto le quattro lingue il segmentato le mostra tutte insieme;
                    oltre, un elenco a tendina — dodici linguette non ci starebbero
                    e nessuno può sapere quante ne troverà questo codice. */}
                {DISPONIBILI.length <= 4 ? (
                  <Segmentato
                    etichetta={t("settings.language.label")}
                    scelta={lingua}
                    onScegli={onLingua}
                    voci={DISPONIBILI.map((l) => ({
                      chiave: l.codice,
                      etichetta: l.nome,
                    }))}
                  />
                ) : (
                  <select
                    className="scegli-lingua"
                    aria-label={t("settings.language.label")}
                    value={lingua}
                    onChange={(e) => onLingua(e.target.value)}
                  >
                    {DISPONIBILI.map((l) => (
                      <option key={l.codice} value={l.codice}>
                        {l.nome}
                      </option>
                    ))}
                  </select>
                )}
              </Scheda>

              <Scheda icona="i-skin" titolo={t("settings.theme.title")} guida="theme">
                <Segmentato
                  etichetta={t("settings.theme.label")}
                  scelta={tema}
                  onScegli={onTema}
                  voci={[
                    /* Una skin mostra la sua base e, se ce l'ha, la variante del
                       tema opposto. Il tema che non sa mostrare si spegne:
                       lasciarlo acceso e non far succedere niente sarebbe una
                       scelta fatta e non avvenuta. */
                    {
                      chiave: "scuro",
                      etichetta: t("settings.theme.dark"),
                      spenta:
                        attiva && attiva.scheme === "light" && !attiva.variante
                          ? t("settings.theme.noDark", { nome: attiva.nome })
                          : undefined,
                    },
                    {
                      chiave: "chiaro",
                      etichetta: t("settings.theme.light"),
                      spenta:
                        attiva && attiva.scheme === "dark" && !attiva.variante
                          ? t("settings.theme.noLight", { nome: attiva.nome })
                          : undefined,
                    },
                    { chiave: "sistema", etichetta: t("settings.theme.system") },
                  ]}
                />
              </Scheda>

              <Scheda
                icona="i-skin"
                titolo={t("settings.skin.title")}
                guida="skin"
                nota={t("settings.skin.installed", { n: skin.length })}
              >
                {/* Prima della griglia, e solo quando è successo: è la risposta
                    alla domanda che porta qui — «perché non è la mia skin?». La
                    scelta nel database è ancora quella, quindi non si dice
                    «cambiata», si dice «non si legge». */}
                {ripiego !== null && (
                  <p className="nota nota-ripiego" role="status">
                    {t("settings.skin.fallback", { id: ripiego })}
                  </p>
                )}
                <div className="griglia-skin">
                  {skin.map((s) => {
                    /* Tre colori scritti dall'autore in `meta.preview`. Quando non
                       ci sono si ripiega sul fondo, sulla superficie e
                       sull'accento — che per una skin sobria dà tre grigi quasi
                       uguali, ed è esattamente il motivo per cui il campo esiste. */
                    const bande =
                      s.anteprima.length === 3
                        ? s.anteprima
                        : ["var(--color-surface-0)", "var(--color-surface-2)", "var(--accent)"];
                    const rotta = s.guasto !== undefined;
                    return (
                      <div
                        key={s.id}
                        className="scheda-skin section-card"
                        data-active={s.attiva || undefined}
                        data-rotta={rotta || undefined}
                        onMouseEnter={() => !s.attiva && !rotta && onAnteprimaSkin(s.id)}
                        onMouseLeave={() => !s.attiva && !rotta && onAnteprimaSkin(null)}
                      >
                        {/* Una skin installata che non passa più la validazione
                            non si indossa e non si prova al passaggio: la scheda
                            porta nello Studio, che è dove si aggiusta. Fino alla
                            3.0 spariva dall'elenco senza dire niente. */}
                        <button
                          type="button"
                          className="prova"
                          aria-label={
                            rotta
                              ? t("settings.skin.openBroken", { nome: s.nome })
                              : t("settings.skin.use", { nome: s.nome })
                          }
                          onClick={() => (rotta ? onApriStudio(s.id) : onScegliSkin(s.id))}
                        >
                          <span className="bande" aria-hidden="true">
                            {bande.map((colore, i) => (
                              <span key={i} style={{ background: colore }} />
                            ))}
                          </span>
                          <span className="nome-skin">{s.nome}</span>
                          <span className="stato">
                            {rotta
                              ? t("settings.skin.broken")
                              : s.attiva
                                ? t("settings.skin.inUse")
                                : s.diSerie
                                  ? t("settings.skin.builtin")
                                  : s.autore}
                          </span>
                        </button>
                        {s.guasto !== undefined && (
                          <p className="descrizione">{tSe(`errors.${s.guasto}`, s.guasto)}</p>
                        )}
                        {s.descrizione !== null && (
                          /* Solo le skin di serie passano dal catalogo delle
                             lingue: la loro descrizione la scriviamo noi, ed è
                             interfaccia come il resto della scheda — «Plain»
                             spiegava sé stessa in italiano dentro
                             un'applicazione in inglese. Quella di una skin
                             installata resta com'è: è un testo di chi l'ha
                             fatta, e tradurlo vorrebbe dire riscriverlo. */
                          <p className="descrizione">
                            {s.diSerie
                              ? tSe(`skin.builtin.${s.id}.desc`, s.descrizione)
                              : s.descrizione}
                          </p>
                        )}
                        {/* «Deriva…» apre la creazione di un tema con questa skin
                            come base, non lo Studio. Prima faceva la stessa cosa
                            di «Apri nello Studio»: si finiva su un documento con
                            «Salva e usa» spento e un suggerimento a cambiare
                            `id` e `meta.name` a mano — un lavoro che la finestra
                            «Crea tema» sapeva già fare, raggiungibile solo
                            dall'altro bottone. */}
                        <div className="azioni-skin">
                          <button
                            type="button"
                            className="bottone minuto btn-ghost"
                            onClick={() =>
                              s.diSerie ? onDeriva(s.id) : onApriStudio(s.id)
                            }
                          >
                            <Icona
                              nome={s.diSerie ? "i-plus" : "i-text"}
                              dim={13}
                            />
                            {s.diSerie
                              ? t("settings.skin.derive")
                              : t("settings.skin.openStudio")}
                          </button>
                          {/* Le skin si disinstallano. `diSerie` porta scritto
                              «non si può disinstallare», il che dice che le altre
                              sì — e il comando non c'era: una skin installata per
                              curiosità restava nell'elenco per sempre. */}
                          {!s.diSerie && (
                            <button
                              type="button"
                              className="bottone minuto btn-ghost via-skin"
                              aria-label={t("settings.skin.uninstall", {
                                nome: s.nome,
                              })}
                              title={
                                s.attiva
                                  ? t("settings.skin.uninstall.active", {
                                      nome: s.nome,
                                    })
                                  : t("settings.skin.uninstall", { nome: s.nome })
                              }
                              onClick={() => onDisinstallaSkin(s.id)}
                            >
                              <Icona nome="i-x" dim={13} />
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
                {/* L'ancora del giro guidato: qui e non sulla griglia sopra,
                    perché il passo racconta lo Studio — e la porta d'ingresso
                    dello Studio per chi non ha ancora nessuna skin sua è «Crea
                    tema», non una delle schede installate. */}
                <div className="azioni" data-giro="studio">
                  {/* Prima di «Installa»: fare un tema è il comando principale di
                      questa scheda, installarne uno fatto da altri è il caso
                      raro. Il pulsante per-scheda qui sopra apre lo Studio su una
                      skin che c'è già; questo ne fa una che non c'è. */}
                  <button
                    type="button"
                    className="bottone primario btn-accent"
                    onClick={onCreaTema}
                  >
                    <Icona nome="i-plus" dim={15} />
                    {t("settings.skin.create")}
                  </button>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={onInstallaSkin}
                  >
                    <Icona nome="i-import" dim={15} />
                    {t("settings.skin.install")}
                  </button>
                  <span className="oppure">{t("settings.skin.orDrop")}</span>
                </div>
              </Scheda>

              {/* In «Aspetto» e non altrove: il giro guidato racconta com'è
                  fatta la finestra, ed è la stessa domanda a cui rispondono la
                  lingua, il tema e le skin. La sezione «Movimento e accesso» —
                  dove stanno le scorciatoie, che il decimo passo nomina —
                  sarebbe stata l'altra candidata, ma là dentro si dichiara una
                  condizione, non si chiede una presentazione. */}
              <Scheda
                icona="i-home"
                titolo={t("settings.tour.title")}
                guida="tour"
              >
                <div className="azioni">
                  <button type="button" className="bottone" onClick={onGiro}>
                    <Icona nome="i-play" dim={15} />
                    {t("settings.tour.redo")}
                  </button>
                </div>
              </Scheda>
            </>
          )}

          {sezione === "riproduzione" && (
            <>
              <Scheda icona="i-eq" titolo={t("settings.eq.title")} guida="eq">
                <Equalizzatore
                  attivo={eqAttivo}
                  guadagni={eqGuadagni}
                  taglia="pagina"
                  onErrore={onErrore}
                />
              </Scheda>
              <Scheda icona="i-play" titolo={t("settings.playback.title")} guida="playback">
                {/* Acceso, finalmente, e senza scorciatoie. La tinta dominante la
                  estrae `aether-app` dalla miniatura del disco; se sia leggibile
                  lo decide `aether-skin` in OKLCH, dove vive `contrast_ratio`.
                  Quando nessuna chiarezza di quella tonalità regge 4,5:1 contro
                  le superfici del tema, la risposta è «no» e vince l'accento
                  della skin: è la ragione per cui questo interruttore è rimasto
                  spento finché non c'è stato un posto in cui metterlo che non
                  fosse «scrivi il colore e spera». */}
                <Interruttore
                  etichetta={t("settings.accent.toggle")}
                  acceso={accentoDinamico && accentoPermesso}
                  onCambia={onAccentoDinamico}
                  impedito={
                    accentoPermesso ? undefined : t("settings.accent.blocked")
                  }
                />
                {/* `normale` di serie, e non è una scelta di disegno: è quel che
                  il motore fa da quando esiste. Un aggiornamento che lo spegnesse
                  in silenzio cambierebbe il volume di chi ha i tag senza che
                  nessuna schermata lo spieghi — ed è la ragione per cui i quattro
                  stati partono da qui e non da «spento». */}
                <Segmentato
                  etichetta={t("settings.replaygain.label")}
                  scelta={replaygain}
                  onScegli={onReplaygain}
                  voci={[
                    { chiave: "spento", etichetta: t("settings.replaygain.off") },
                    { chiave: "basso", etichetta: t("settings.replaygain.low") },
                    {
                      chiave: "normale",
                      etichetta: t("settings.replaygain.normal"),
                    },
                    { chiave: "alto", etichetta: t("settings.replaygain.high") },
                  ]}
                />
                <VolumeUniforme onErrore={onErrore} />
                <Interruttore
                  etichetta={t("settings.autoplay.toggle")}
                  acceso={autoplay}
                  onCambia={onAutoplay}
                />
                {/* Qui e non in «Aspetto», per la regola di questa sezione: il
                    dominio della chiave è `player.*`. E dentro questa scheda e
                    non in una sua, perché una scheda intera per un interruttore
                    solo sarebbe un titolo più alto di quel che contiene. */}
                <InterruttoreFormato onErrore={onErrore} />
              </Scheda>
              {/* Prima della dissolvenza e dopo l'equalizzatore, che è l'ordine
                  di quanto spesso ci si torna: da quale scheda esce il suono è
                  la prima cosa che si viene a cercare qui quando qualcosa non si
                  sente, e la dissolvenza è una preferenza che si decide una
                  volta. */}
              <Scheda icona="i-play" titolo={t("settings.output.title")} guida="output">
                <SchedaUscita onErrore={onErrore} />
              </Scheda>
              <SchedaDissolvenza
                dissolvenzaS={dissolvenzaS}
                onDissolvenza={onDissolvenza}
              />
              <SchedaSpegnimento
                spegnimentoMs={spegnimentoMs}
                onSpegnimento={onSpegnimento}
              />
              {/* In «Riproduzione» e non in «Aspetto»: il dominio delle
                  chiavi è `player.*`, come ogni altro residente di questa
                  sezione. «Aspetto» è dove vive il look — che per la linea di
                  questa scheda è esattamente la metà che non è
                  un'impostazione, ma un blocco di token dello Studio. */}
              <Scheda icona="i-eq" titolo={t("settings.spectrum.title")} guida="spectrum">
                <SchedaSpettro onErrore={onErrore} />
              </Scheda>
            </>
          )}

          {sezione === "movimento" && (
            <Scheda
              icona="i-eq"
              titolo={t("settings.shortcuts.title")}
              guida="shortcuts"
            >
              <Scorciatoie associazioni={scorciatoie} onCambia={onScorciatoie} />
            </Scheda>
          )}

          {/* Fra le scorciatoie e il movimento, e in questa sezione e non in
              «Aspetto»: ingrandire non è una scelta di look come il tema o la
              skin — è la stessa domanda del movimento ridotto, cioè «fai in modo
              che io ci arrivi». Sta sotto le scorciatoie perché è la scheda che
              le spiega: quelle tre righe lì sopra dicono i tasti, questi tre
              bottoni fanno la stessa cosa per chi i tasti li ha tolti. */}
          {sezione === "movimento" && (
            <Scheda
              icona="i-expand"
              titolo={t("settings.zoom.title")}
              guida="zoom"
            >
              {/* I tre bottoni e non un cursore o un elenco di misure: un
                  elenco vorrebbe i gradini scritti da questa parte, cioè un
                  secondo elenco di numeri accanto a quello del nucleo. Qui si
                  mandano gesti, e il numero che si legge è quello che il nucleo
                  ha risposto. */}
              <div className="azioni">
                <button
                  type="button"
                  className="bottone"
                  onClick={() => onZoom(false)}
                >
                  {t("settings.zoom.smaller")}
                </button>
                <button type="button" className="bottone" onClick={onZoomNormale}>
                  {t("settings.zoom.reset")}
                </button>
                <button
                  type="button"
                  className="bottone"
                  onClick={() => onZoom(true)}
                >
                  {t("settings.zoom.bigger")}
                </button>
              </div>
              <p className="nota">
                {t("settings.zoom.now", { quanto: Math.round(zoom * 100) })}
              </p>
            </Scheda>
          )}

          {sezione === "movimento" && (
            <Scheda icona="i-eq" titolo={t("settings.motion.title")} guida="motion">
              <p className="nota">
                <Trans
                  k="settings.motion.p1"
                  v={{
                    livello: <strong>{movimento}</strong>,
                  }}
                />
              </p>

              {/* Due voci e non tre. Da qui si può soltanto **ridurre** sotto
                  quel che la skin dichiara: un livello «di più» contraddirebbe
                  la frase qui sopra, che dice che una preferenza di
                  accessibilità sovrascrivibile non è una preferenza. Chi vuole
                  più movimento cambia skin, che è dove il movimento si
                  dichiara. */}
              <Segmentato
                etichetta={t("settings.motion.pref")}
                scelta={movimentoUtente}
                onScegli={onMovimentoUtente}
                voci={[
                  { chiave: "sistema", etichetta: t("settings.motion.system") },
                  { chiave: "ridotto", etichetta: t("settings.motion.reduce") },
                ]}
              />
            </Scheda>
          )}

          {sezione === "nuvola" && (
            <Scheda icona="i-cloud" titolo={t("settings.cloud.title")} guida="cloud">

              {nuvola?.errore && (
                <div className="errore">{testoErrore(nuvola.errore)}</div>
              )}

              {/* Due frasi e non una: «non configurato» da solo sembrava un
                  guasto, e non lo è — è una scelta di come Aether viene
                  distribuito. La prima dice cosa vuol dire, la seconda cosa si
                  può fare, compresa la strada che non chiede nessun account. */}
              {nuvola && !nuvola.configurato && (
                <>
                  <p className="nota">{t("settings.cloud.noCreds")}</p>
                </>
              )}

              <Interruttore
                etichetta={t("settings.cloud.toggle")}
                acceso={nuvola?.attivo ?? false}
                onCambia={onNuvolaAttiva}
                impedito={
                  nuvola?.collegato ? undefined : t("settings.cloud.needAccount")
                }
              />

              <dl className="numeri">
                <div>
                  <dt>{t("settings.cloud.account")}</dt>
                  <dd className="stat-number">
                    {nuvola?.email ??
                      (nuvola?.collegato
                        ? t("settings.cloud.connected")
                        : t("settings.cloud.none"))}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.cloud.lastSave")}</dt>
                  <dd className="stat-number">
                    {quando(nuvola?.ultimoMs ?? null)}
                  </dd>
                </div>
              </dl>

              {avanzaNuvola && (
                <div className="avanzamento-scansione">
                  <div className="riga-avanzamento">
                    <span className="quanti">
                      {fasiNuvola()[avanzaNuvola.cosa]}{" "}
                      {numero(avanzaNuvola.fatti)} / {numero(avanzaNuvola.totale)}
                    </span>
                  </div>
                  <div className="barra player-progress">
                    <div
                      className="riempita"
                      style={{
                        width: `${
                          avanzaNuvola.totale > 0
                            ? Math.round(
                                (avanzaNuvola.fatti / avanzaNuvola.totale) * 100,
                              )
                            : 0
                        }%`,
                      }}
                    >
                      <span className="riflesso progress-sheen" aria-hidden="true" />
                    </div>
                  </div>
                </div>
              )}

              <div className="azioni">
                {nuvola?.collegato ? (
                  <>
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      disabled={nuvola.inCorso}
                      onClick={onNuvolaSalva}
                    >
                      <Icona nome="i-cloud" dim={15} />
                      {nuvola.inCorso
                        ? t("settings.cloud.saving")
                        : t("settings.cloud.saveNow")}
                    </button>
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      disabled={nuvola.inCorso}
                      onClick={onNuvolaRipristina}
                    >
                      <Icona nome="i-import" dim={15} />
                      {t("settings.cloud.restore")}
                    </button>
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      disabled={nuvola.inCorso}
                      onClick={onNuvolaScollega}
                    >
                      {t("settings.cloud.disconnect")}
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={!nuvola?.configurato || nuvola.inCorso}
                    onClick={onNuvolaCollega}
                  >
                    <Icona nome="i-cloud" dim={15} />
                    {nuvola?.inCorso
                      ? t("settings.cloud.waiting")
                      : t("settings.cloud.connect")}
                  </button>
                )}
                {/* Chi ha aperto il consenso per sbaglio, o ha chiuso la scheda
                    del browser, non deve aspettare tre minuti davanti a un tasto
                    spento. Il collegamento annullato torna senza errore. */}
                {nuvola?.inCorso && !nuvola.collegato && (
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={() => {
                      ipc.nuvolaAnnulla().catch(onErrore);
                    }}
                  >
                    {t("settings.cloud.cancel")}
                  </button>
                )}
              </div>

              {/* In chiaro, e non più dietro un `<details>` intitolato «Usa il
                  tuo progetto Google».
                  Quel titolo prometteva un lusso — «solo se vuoi che la quota sia
                  la tua» — mentre queste due caselle sono l'unica cosa che fa
                  esistere il backup: senza, la scheda qui sopra dichiara che non
                  può partire. Quel che tiene in piedi una funzione non si mette
                  dietro una freccetta. */}
              <div className="credenziali-drive">
                <h3 className="titoletto">{t("settings.creds.title")}</h3>

                <div className="azioni">
                  <input
                    type="text"
                    className="campo mono field-input"
                    aria-label={t("settings.creds.id")}
                    placeholder="client id"
                    value={clientId}
                    onChange={(e) => setClientId(e.target.value)}
                  />
                  <input
                    type="password"
                    className="campo mono field-input"
                    aria-label={t("settings.creds.secret")}
                    placeholder="client secret"
                    value={clientSecret}
                    onChange={(e) => setClientSecret(e.target.value)}
                  />
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={() => onNuvolaCredenziali(clientId, clientSecret)}
                  >
                    {t("settings.creds.use")}
                  </button>
                </div>
                {/* Detto come quel che è — un modo di **togliere** — e non come
                    un ripiego su credenziali che in questa copia non esistono. */}

                {/* Il tasto e non un `<details>`: sono sei passi da fare una
                    volta sola, e chi torna qui torna per incollare. */}
                <div className="azioni">
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    aria-expanded={guidaDrive}
                    onClick={() => setGuidaDrive((prima) => !prima)}
                  >
                    <Icona nome={guidaDrive ? "i-chev-d" : "i-list"} dim={15} />
                    {guidaDrive
                      ? t("settings.creds.hide")
                      : t("settings.creds.show")}
                  </button>
                </div>

                {guidaDrive && (
                  <>
                    {/* Numerati, come i tre passi del Client ID di Spotify in
                        `Account.tsx`, e per la stessa ragione: sono cose da fare
                        **in ordine** su un sito che non è questo, e chi torna dal
                        browser deve ritrovare il punto in cui era. */}
                    <ol className="passi">
                      <li>
                        <Trans
                          k="settings.creds.step1"
                          v={{ console: <code>console.cloud.google.com</code> }}
                        />
                      </li>
                      <li>
                        <Trans
                          k="settings.creds.step2"
                          v={{
                            dove: (
                              <strong>{t("settings.creds.step2.where")}</strong>
                            ),
                            api: <strong>{t("settings.creds.step2.api")}</strong>,
                          }}
                        />
                      </li>
                      <li>
                        <Trans
                          k="settings.creds.step3"
                          v={{
                            dove: (
                              <strong>{t("settings.creds.step3.where")}</strong>
                            ),
                            tipo: (
                              <strong>{t("settings.creds.step3.type")}</strong>
                            ),
                            pubblica: (
                              <strong>{t("settings.creds.step3.publish")}</strong>
                            ),
                          }}
                        />
                      </li>
                      <li>
                        <Trans
                          k="settings.creds.step4"
                          v={{ scope: <code>.../auth/drive.appdata</code> }}
                        />
                      </li>
                      <li>
                        <Trans
                          k="settings.creds.step5"
                          v={{
                            dove: (
                              <strong>{t("settings.creds.step5.where")}</strong>
                            ),
                            tipo: (
                              <strong>{t("settings.creds.step5.type")}</strong>
                            ),
                          }}
                        />
                      </li>
                      <li>
                        <Trans
                          k="settings.creds.step6"
                          v={{
                            id: <strong>{t("settings.creds.step6.id")}</strong>,
                            secret: (
                              <strong>{t("settings.creds.step6.secret")}</strong>
                            ),
                          }}
                        />
                      </li>
                    </ol>
                  </>
                )}
              </div>
            </Scheda>
          )}

          {sezione === "sincronia" && (
            <Scheda icona="i-cloud" titolo={t("settings.sync.title")} guida="sync">
              {/* La differenza dal backup, detta con l'esempio invece che con la
                  definizione: «una copia da cui ripartire» e «dispositivi che
                  continuano a vivere ciascuno per conto suo» sono vere e non
                  dicono cosa succede a chi accende la cosa sbagliata. */}

              {sincronia?.errore && (
                <div className="errore">{testoErrore(sincronia.errore)}</div>
              )}

              <Interruttore
                etichetta={t("settings.sync.toggle")}
                acceso={sincronia?.attiva ?? false}
                onCambia={onSincroniaAttiva}
                impedito={
                  sincronia?.pronta ? undefined : t("settings.sync.needStore")
                }
              />

              <div className="azioni">
                <Segmentato
                  etichetta={t("settings.sync.where")}
                  scelta={sincronia?.dove ?? "cartella"}
                  voci={[
                    {
                      chiave: "cartella",
                      etichetta: t("settings.sync.where.folder"),
                    },
                    {
                      chiave: "drive",
                      etichetta: t("settings.sync.where.drive"),
                    },
                  ]}
                  onScegli={(dove) =>
                    onSincroniaMagazzino(
                      dove,
                      cartellaSincronia ?? sincronia?.cartella ?? null,
                    )
                  }
                />
              </div>

              {(sincronia?.dove ?? "cartella") === "cartella" ? (
                <>
                  <div className="azioni">
                    <input
                      type="text"
                      className="campo field-input"
                      placeholder={t("settings.sync.folder.hint")}
                      value={cartellaSincronia ?? sincronia?.cartella ?? ""}
                      onChange={(e) => setCartellaSincronia(e.target.value)}
                    />
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      onClick={() => {
                        onSincroniaMagazzino(
                          "cartella",
                          cartellaSincronia ?? sincronia?.cartella ?? "",
                        );
                        setCartellaSincronia(null);
                      }}
                    >
                      {t("settings.sync.folder.use")}
                    </button>
                  </div>
                </>
              ) : (
                sincronia?.pronta === false && (
                  <p className="nota">{t("settings.sync.drive.needAccount")}</p>
                )
              )}

              <dl className="numeri">
                <div>
                  <dt>{t("settings.sync.thisDevice")}</dt>
                  <dd className="stat-number">
                    {sincronia?.dispositivi.find((d) => d.sonoIo)?.nome ??
                      sincronia?.io ??
                      "—"}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.sync.lastPass")}</dt>
                  <dd className="stat-number">
                    {quando(sincronia?.ultimaMs ?? null)}
                  </dd>
                </div>
                <div>
                  <dt>{t("settings.data.devices")}</dt>
                  <dd className="stat-number">
                    {numero(sincronia?.dispositivi.length ?? 0)}
                  </dd>
                </div>
              </dl>

              {sincronia?.resoconto && (
                <p className="nota">{riassunto(sincronia.resoconto)}</p>
              )}

              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={!sincronia?.pronta || sincronia.inCorso}
                  onClick={onSincroniaAdesso}
                >
                  <Icona nome="i-cloud" dim={15} />
                  {sincronia?.inCorso
                    ? t("settings.sync.syncing")
                    : t("settings.sync.now")}
                </button>
              </div>

              {(sincronia?.dispositivi.length ?? 0) > 0 && (
                <details className="non-ritrovati">
                  <summary>
                    {t("settings.sync.devices", {
                      n: sincronia?.dispositivi.length ?? 0,
                    })}
                  </summary>
                  <ul className="cartelle">
                    {(sincronia?.dispositivi ?? []).map((dispositivo) => (
                      <li key={dispositivo.id} className="azioni">
                        {dispositivo.sonoIo ? (
                          <span className="percorso">
                            {t("settings.sync.thisComputer", {
                              nome: dispositivo.nome ?? dispositivo.id,
                            })}
                          </span>
                        ) : (
                          <input
                            type="text"
                            className="campo field-input"
                            aria-label={t("settings.sync.deviceName", {
                              id: dispositivo.id,
                            })}
                            /* Il campo è la sola cosa che serve, e non una
                               finestrella: `window.prompt` in una webview è un
                               modale che il sistema può decidere di non mostrare,
                               e un pulsante che a volte non fa niente è peggio di
                               un campo brutto. */
                            placeholder={dispositivo.id}
                            value={nomi[dispositivo.id] ?? dispositivo.nome ?? ""}
                            onChange={(e) =>
                              setNomi({
                                ...nomi,
                                [dispositivo.id]: e.target.value,
                              })
                            }
                          />
                        )}
                        {!dispositivo.sonoIo && (
                          <>
                            <button
                              type="button"
                              className="bottone btn-ghost"
                              onClick={() =>
                                onSincroniaAccoppia(
                                  dispositivo.id,
                                  nomi[dispositivo.id] ??
                                    dispositivo.nome ??
                                    null,
                                )
                              }
                            >
                              {dispositivo.fidato
                                ? t("settings.sync.rename")
                                : t("settings.sync.accept")}
                            </button>
                            <button
                              type="button"
                              className="bottone btn-ghost"
                              onClick={() => onSincroniaDimentica(dispositivo.id)}
                            >
                              {t("settings.sync.forget")}
                            </button>
                          </>
                        )}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
            </Scheda>
          )}
          {sezione === "legacy" && (
            <Scheda icona="i-import" titolo={t("settings.legacy.title")} guida="legacy">
              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={onImporta}
                >
                  <Icona nome="i-import" dim={15} />
                  {t("settings.legacy.pick")}
                </button>
              </div>
            </Scheda>
          )}

          {/* Una scheda sola, e due righe.
              C'era un selettore Spotify/YouTube in cima, e non c'è più: da
              YouTube non si scarica — le loro *API Developer Policies*
              (§ III.E.1.a) lo vietano esplicitamente, e non c'è configurazione
              che lo renda lecito — quindi la domanda «da quale servizio» non ha
              più due risposte. Restano due gesti diversi: un link, o il proprio
              archivio. */}
          {sezione === "esterno" && (
            <Scheda
              icona="i-list"
              titolo={t("settings.import.title")}
              guida="import"
            >
              <div className="strade-import">
                <div className="riga-opzione">
                  <div className="che-cosa">
                    <div className="etichetta">{t("settings.import.link")}</div>
                  </div>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={onImportaLink}
                  >
                    <Icona nome="i-list" dim={15} />
                    {t("settings.import.link.cta")}
                  </button>
                </div>

                <div className="riga-opzione">
                  <div className="che-cosa">
                    <div className="etichetta">
                      {t("settings.import.archive")}
                    </div>
                  </div>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={onImportaAccount}
                  >
                    <Icona nome="i-cloud" dim={15} />
                    {t("settings.import.archive.cta")}
                  </button>
                </div>
              </div>

              {/* Le fonti attive, con la loro licenza e quel che si può farne.
                  Non è decorazione: è la risposta alla domanda che chiunque
                  distribuisca musica ha il diritto di farsi — da dove viene quel
                  che questa applicazione mi mette sul disco. */}
              <ul className="fonti-attive">
                {(importazioni.stato?.cataloghi ?? []).map((catalogo) => (
                  <li key={catalogo.nome}>
                    <span className="etichetta">{nomeFonte(catalogo.nome)}</span>
                    <span className="spiegazione">
                      {catalogo.consegna
                        ? t("settings.import.source.download")
                        : t("settings.import.source.listen")}
                    </span>
                  </li>
                ))}
              </ul>

              <Interruttore
                etichetta={t("settings.import.alt")}
                acceso={importazioni.stato?.alternative ?? true}
                onCambia={(v) => void importazioni.ammettiAlternative(v)}
              />

              {/* Un rimando e non più il pannello intero.
                  Quel che succede dopo la conferma dura **un'ora** e sopravvive
                  alla chiusura dell'applicazione: il settimo pannello di nove
                  dentro le impostazioni non è un posto dove si torna a guardare
                  come va — è un posto in cui si va a cambiare qualcosa. Il
                  conteggio resta qui perché la porta senza un numero dietro è una
                  porta che non si apre; l'elenco sta nella pagina. */}
              {inCoda > 0 ? (
                <button
                  type="button"
                  className="bottone rimando"
                  onClick={() => onVista("importazioni")}
                >
                  <Icona nome="i-list" dim={15} />
                  {t("settings.queue.link", { n: inCoda })}
                  {importazioni.stato?.attiva === false &&
                    t("settings.queue.paused")}
                  <span className="freccia" aria-hidden="true">
                    ›
                  </span>
                </button>
              ) : (
                <p className="nota">
                  <Trans
                    k="settings.queue.empty"
                    v={{
                      porta: (
                        <button
                          type="button"
                          className="tasto-testo"
                          onClick={() => onVista("importazioni")}
                        >
                          {t("settings.queue.empty.link")}
                        </button>
                      ),
                    }}
                  />
                </p>
              )}
            </Scheda>
          )}
          {sezione === "scrobbling" && (
            <Scheda
              icona="i-cloud"
              titolo={t("settings.scrobbling.title")}
              guida="scrobbling"
            >
              <Scrobbling onErrore={onErrore} onNotizia={onNotizia} />
            </Scheda>
          )}
          {sezione === "modelli" && (
            <Scheda
              icona="i-ia"
              titolo={t("settings.models.title")}
              guida="models"
            >
              <ModelliIA onErrore={onErrore} onNotizia={onNotizia} />
            </Scheda>
          )}
          {sezione === "chiusura" && <SchedaChiusura onErrore={onErrore} />}

          {sezione === "dati" && avvio && (
            <Scheda icona="i-album" titolo={t("settings.data.title")} guida="data">
              <dl className="numeri">
                <div>
                  <dt>{t("settings.data.tracks")}</dt>
                  <dd className="stat-number">{numero(avvio.numeri.tracks)}</dd>
                </div>
                <div>
                  <dt>{t("settings.data.albums")}</dt>
                  <dd className="stat-number">{numero(avvio.numeri.albums)}</dd>
                </div>
                <div>
                  <dt>{t("settings.data.artists")}</dt>
                  <dd className="stat-number">{numero(avvio.numeri.artists)}</dd>
                </div>
                <div>
                  <dt>{t("settings.data.listening")}</dt>
                  <dd className="stat-number">{ore(avvio.numeri.durationMs)}</dd>
                </div>
              </dl>
              <p className="percorso" title={avvio.dataDir}>
                {avvio.dataDir}
              </p>
              <p className="nota">
                {avvio.migrazioni === 0
                  ? t("settings.data.noMigrations")
                  : t("settings.data.migrations", { n: avvio.migrazioni })}{" "}
                {avvio.fts5
                  ? t("settings.data.fts.ok")
                  : t("settings.data.fts.missing")}
              </p>
            </Scheda>
          )}

          {sezione === "dati" && <SchedaManutenzione onErrore={onErrore} />}

          {sezione === "dati" && (
            <Scheda
              icona="i-import"
              titolo={t("settings.profile.title")}
              guida="profile"
            >
              <Profilo
                onErrore={onErrore}
                onNotizia={onNotizia}
                onImportato={onProfiloImportato}
              />
            </Scheda>
          )}

          {sezione === "aggiornamenti" && (
            <Scheda
              icona="i-cloud"
              titolo={t("settings.update.title")}
              guida="update"
            >
              <Aggiornamenti onErrore={onErrore} />
            </Scheda>
          )}
        </div>
      </div>
    </ErroreGuida.Provider>
  );
}
