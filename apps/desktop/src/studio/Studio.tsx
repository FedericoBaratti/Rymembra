/**
 * Lo Skin Studio.
 *
 * # La tesi
 *
 * Una skin è un documento di dati validato. `parse_skin` lo legge, `check_skin`
 * avvisa, `compile_skin` lo trasforma in CSS deterministico. Tutto quel che
 * serve a un editor **esiste già nel crate**: settantatré token con la loro
 * descrizione, i loro estremi e il flag di obbligatorietà, cinquantuno parti
 * coi loro gruppi, undici effetti col costo dichiarato, un vocabolario chiuso
 * per parte, e `nearest_parts()` per i refusi.
 *
 * Lo Studio non aggiunge potere: rende visibile un contratto che oggi si scopre
 * leggendo Rust.
 *
 * # Le tre regole
 *
 * 1. **Non si può scrivere quel che il formato non accetta.** Ogni controllo ha
 *    la forma di un tipo del crate — un segmentato per un enum, un cursore per
 *    una lunghezza — e non c'è nessun campo di testo libero dove il documento
 *    vuole un valore chiuso. Il posto dove si scrive liberamente è uno solo, ed
 *    è la scheda «Documento»: lì è il parser a rispondere.
 * 2. **L'anteprima è l'applicazione, non un riquadro.** L'anteprima vera sta in
 *    Impostazioni — passare sopra la scheda di una skin ridipinge la finestra
 *    intera con `skin(id)`, che è un comando che esisteva già e faceva
 *    esattamente questo. La miniatura qui dentro serve alla sonda, che ha
 *    bisogno di una superficie ferma su cui passare il puntatore.
 * 3. **Il costo si vede mentre lo si spende.** Ogni effetto porta il suo peso
 *    accanto al nome, e la superficie ha un contatore su dieci.
 */
import { save } from "@tauri-apps/plugin-dialog";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import {
  ipc,
  type Istantanea,
  type NodoScafale,
  type PresetRegistro,
  type Registro,
  type Validazione,
  type VoceFile,
} from "../ipc";
import { Impaginazione } from "../Impaginazione";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { Anteprima, MISURA } from "./Anteprima";
import { Chat } from "./Chat";
import { Documento, type Scheda } from "./Documento";
import { Ispettore } from "./Ispettore";
import { IspettoreNodo, Scafale } from "./Scafale";
import { Pacchetto } from "./Pacchetto";
import { Tavolozza } from "./Tavolozza";
import { Token } from "./Token";
import {
  contestoFinto,
  effettive,
  pagine,
  sovrapposizioni,
  spentaPerche,
  type Pagina,
  type Sovrapposizione,
} from "./finto";
import {
  DOVE_SI_VEDE,
  NON_ANCORA,
  Sovrapposte,
  perchePartMai,
  slotDellaPagina,
} from "./scene";
import { useStoria } from "./storia";
import {
  leggi,
  percorsoParte,
  rinominaChiave,
  scrivi as riscrivi,
  scriviIn,
  togliDa,
  valoreIn,
  vuoto,
} from "./patch";
import {
  conNodo,
  corpoPrefab,
  nodoA,
  nomePrefabLibero,
  versoDocumento,
  type Via,
} from "./albero";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import {
  descrizioneParte,
  descrizioneToken,
  nomeGruppo,
} from "./vocabolario";

/** Le quattro viste dello Studio. */
type Vista = "ispeziona" | "impagina" | "documento" | "tavolozza";

/**
 * I gradini dell'ingrandimento.
 *
 * `null` è «stai nello spazio che c'è», cioè la miniatura di sempre. Gli altri
 * due sono misure vere: a `1` un bordo da un pixel è un pixel, ed è l'unica
 * scala a cui si possa giudicare un raggio da tre o una hairline.
 */
function ingrandimenti(): readonly (readonly [string, number | null])[] {
  return [
    [t("studio.zoom.fit"), null],
    ["1:1", 1],
    ["2×", 2],
  ];
}

/**
 * La testata dell'anteprima: quale pagina, e cosa c'è sopra.
 *
 * Due righe e non una, perché sono due assi: sotto le linguette della pagina
 * stanno gli interruttori delle sovrapposizioni, che si accendono a piacere e
 * in qualunque combinazione l'applicazione sappia produrre. Un interruttore che
 * qui non avrebbe effetto — la coda a schermo intero, la colonna senza un brano
 * — si spegne **e dice perché**, invece di accendersi e non far succedere
 * niente: quella è la stessa ambiguità che tutta questa vista esiste per non
 * avere.
 *
 * Le due righe portano la larghezza misurata del riquadro, come già faceva la
 * prima: finiscono dove finisce quel che governano.
 */
function TestaScene({
  larghezza,
  pagina,
  accese,
  onPagina,
  onCommuta,
  children,
}: {
  larghezza: number;
  pagina: Pagina;
  /**
   * Quel che è acceso, **prima** del filtro di `effettive()`.
   *
   * Gli interruttori ricordano: si accende la coda, si passa a schermo intero
   * dove non può esserci, si torna indietro e la coda è ancora accesa. Spegnerli
   * davvero avrebbe fatto pagare il viaggio di andata e ritorno.
   */
  accese: ReadonlySet<Sovrapposizione>;
  onPagina: (pagina: Pagina) => void;
  onCommuta: (quale: Sovrapposizione) => void;
  /** Quel che va a destra della prima riga: la scala, o l'indirizzo del nodo. */
  children?: ReactNode;
}) {
  return (
    <>
      {/* La testata è larga quanto il riquadro, misurato: le linguette stanno
          sopra la scena che cambiano, e la riga di sotto finisce dove finisce
          quel che misura.

          Le nove pagine hanno la prima riga tutta per sé. Dividerla con la
          scala voleva dire che le ultime due — «Vuoto» e «Caricamento» —
          uscivano dal bordo su una finestra normale: raggiungibili scorrendo, e
          quindi invisibili, che per un elenco di destinazioni è come non
          averle. */}
      <div className="testa-centro" style={{ width: larghezza }}>
        <Segmentato
          etichetta={t("studio.whichScreen")}
          scelta={pagina}
          onScegli={onPagina}
          classe="minuto"
          voci={pagine().map(([chiave, etichetta]) => ({ chiave, etichetta }))}
        />
        <span className="spinta" />
      </div>

      <div className="testa-sovrapposte" style={{ width: larghezza }}>
        <span className="titolino">{t("studio.whichOverlays")}</span>
        {sovrapposizioni().map(([chiave, etichetta]) => {
          const perche = spentaPerche(chiave, pagina, accese);
          const accesa = accese.has(chiave);
          return (
            <button
              key={chiave}
              type="button"
              className="chip-gruppo"
              role="switch"
              aria-checked={accesa && perche === undefined}
              data-active={(accesa && perche === undefined) || undefined}
              disabled={perche !== undefined}
              title={perche}
              onClick={() => onCommuta(chiave)}
            >
              {etichetta}
            </button>
          );
        })}
        <span className="spinta" />
        {/* La scala e la sua lettura stanno qui, all'angolo destro della riga
            che tocca il riquadro: è dove finisce quel che misurano. */}
        {children}
      </div>
    </>
  );
}

export function Studio({
  id,
  onEsci,
  onInstallata,
  onModelli,
  onErrore,
}: {
  /** La skin da aprire: una bozza, una installata, o quella di serie. */
  id: string;
  onEsci: () => void;
  /** La skin è stata installata da qui: la finestra intera va ridipinta. */
  onInstallata: (id: string) => void;
  /**
   * Esci dallo Studio e apri Impostazioni › Modelli IA.
   *
   * Serve alla chat quando non c'è nessun modello configurato: dire «configurane
   * uno» senza portarci lascerebbe da cercare la scheda in una finestra che
   * questa nasconde per intero.
   */
  onModelli: () => void;
  onErrore: (e: unknown) => void;
}) {
  const [registro, setRegistro] = useState<Registro | null>(null);
  /**
   * Il testo, con la sua storia.
   *
   * `setSorgente` è diventato `scriviSorgente` per tutti allo stesso modo:
   * controlli, editor e ripristini passano di qui, quindi non esiste una
   * modifica che sfugga all'annullo. Il caricamento del documento è l'unica
   * eccezione, e usa `riparti` — vedi `storia.ts`.
   */
  const {
    sorgente,
    scriviSorgente: setSorgente,
    riparti,
    annulla,
    ripeti,
    puoAnnullare,
    puoRipetere,
  } = useStoria();
  const [originale, setOriginale] = useState("");
  /**
   * Il testo che sta nella bozza su disco, se ce n'è una.
   *
   * È diverso da `originale`, che è quel che sta nel pacchetto installato. I due
   * rispondono a due domande che si somigliano e non sono la stessa: «ho
   * cambiato qualcosa rispetto alla skin installata» e «quel che ho scritto è al
   * sicuro». La seconda era rimasta senza risposta, e la pillola «non salvata»
   * la dava sbagliata.
   */
  const [salvataAl, setSalvataAl] = useState("");
  const [esito, setEsito] = useState<Validazione | null>(null);
  const [ultimoValido, setUltimoValido] = useState<Validazione | null>(null);
  const [vista, setVista] = useState<Vista>("ispeziona");
  const [scheda, setScheda] = useState<Scheda>("json");
  /**
   * I due assi dell'anteprima: quale pagina, e cosa c'è sopra.
   *
   * Era un valore solo, e mescolava due cose che nell'app sono indipendenti: la
   * scena «modale» *era* la coda aperta più due brani selezionati, e non c'era
   * modo di guardare la notifica sopra Impostazioni — che è l'unico posto in cui
   * la notifica della scansione compare davvero.
   */
  const [pagina, setPagina] = useState<Pagina>("libreria");
  const [accese, setAccese] = useState<ReadonlySet<Sovrapposizione>>(
    () => new Set(),
  );
  const [sonda, setSonda] = useState(true);
  const [parteScelta, setParteScelta] = useState<string | null>(null);
  /**
   * Il token aperto nell'editor.
   *
   * È il gemello di `parteScelta`, e per un pezzo non è esistito: l'albero
   * elencava i token e cliccarli non faceva niente, quindi per cambiare
   * `color.accent` bisognava scendere nel JSON.
   */
  const [tokenScelto, setTokenScelto] = useState<string | null>(null);
  const [stato, setStato] = useState<string | null>(null);
  /**
   * Quante volte si è chiesto di rigiocare la scena.
   *
   * È una `key` su `<Impaginazione>`, e serve a una cosa sola: **un'animazione
   * al montaggio non si vede mai se la scena non si rimonta.** Un'animazione su
   * `enter` parte quando l'elemento entra nel documento, cioè una volta; da lì
   * in poi si può ridipingere la skin quanto si vuole e quella regola resta
   * ferma dov'era finita. Cambiare la chiave smonta e rimonta il sottoalbero,
   * che è l'unico modo che il CSS offre di far ripartire un `animation` senza
   * toccarlo.
   *
   * Un numero e non un `boolean` che si commuta: il rimontaggio deve poter
   * succedere due volte di seguito, e un valore che alterna fra due stati
   * darebbe la seconda pressione uguale alla prima ogni volta che le due si
   * accorpano nello stesso disegno.
   */
  const [rigiocata, setRigiocata] = useState(0);
  const [filtro, setFiltro] = useState("");
  const [lato, setLato] = useState<"token" | "parti">("parti");
  /** Quale variante mostra l'anteprima. Cambia il riquadro, non la finestra. */
  const [tema, setTema] = useState<"dark" | "light">("dark");
  /** A che scala si guarda. `null` è «adatta allo spazio». */
  const [ingrandimento, setIngrandimento] = useState<number | null>(null);
  /**
   * Quante volte la parte scelta compare nella scena aperta.
   *
   * Serve a dire «questa parte qui non c'è, sta di là». Prima non lo diceva
   * nessuno: si sceglieva `toast-card`, si ridipingeva, non succedeva niente, e
   * non c'era modo di distinguere una skin che non funziona da una superficie
   * che non è in questa scena.
   */
  const [quanteVolte, setQuanteVolte] = useState(0);
  /**
   * Quanto è larga l'anteprima adesso, in pixel: la misura la prende lei.
   *
   * Serve a due cose che devono dire la stessa: la lettura «1280×820 · 78%» e
   * la larghezza della testata, che sta sopra il riquadro e deve finire dove
   * finisce lui. Parte dalla misura piena perché al primo disegno il riquadro
   * non è ancora stato misurato, e una testata larga zero si vedrebbe.
   */
  const [larghezza, setLarghezza] = useState<number>(MISURA.larghezza);
  /** I gruppi aperti nell'albero. Chiusi tutti, meno quello che si sta usando. */
  const [aperti, setAperti] = useState<ReadonlySet<string>>(new Set());
  const [fileDelPacchetto, setFileDelPacchetto] = useState<VoceFile[]>([]);
  const [istantanee, setIstantanee] = useState<Istantanea[]>([]);
  /** Il percorso su cui portare il cursore, alla prossima apertura del testo. */
  const [vaiAlla, setVaiAlla] = useState<string | null>(null);
  /** Come `vaiAlla`, ma per quel che una riga ce l'ha e un percorso no. */
  const [vaiAllaRiga, setVaiAllaRiga] = useState<number | null>(null);
  /** Il nodo scelto nella vista Impagina, come percorso di indici. */
  const [nodoScelto, setNodoScelto] = useState<Via | null>(null);
  /** L'indirizzo del nodo sotto il puntatore: la seconda briciola di pane. */
  const [nodoSotto, setNodoSotto] = useState<string | null>(null);
  /**
   * La chat è aperta.
   *
   * Chiusa di serie, e non è timidezza: il pannello prende trecentosessanta
   * pixel all'anteprima, che è la cosa che si sta guardando. Chi non ha
   * configurato nessun modello non deve pagarli mai.
   */
  const [chat, setChat] = useState(false);

  /** La stessa misura, letta come percentuale della finestra vera. */
  const scala = Math.round((larghezza / MISURA.larghezza) * 100);

  useEffect(() => {
    ipc.studioRegistro().then(setRegistro).catch(onErrore);
  }, [onErrore]);

  useEffect(() => {
    ipc
      .studioDocumento(id)
      .then((testo) => {
        riparti(testo);
        setOriginale(testo);
        // Quel che si è appena letto viene dal disco per definizione, che ci sia
        // arrivato da una bozza o dal pacchetto.
        setSalvataAl(testo);
      })
      .catch(onErrore);
  }, [id, onErrore, riparti]);

  /** L'albero del pacchetto: cambia solo quando si esporta o si installa. */
  const rileggiPacchetto = useCallback(() => {
    ipc.studioPacchetto(id).then(setFileDelPacchetto).catch(onErrore);
    ipc.studioIstantanee(id).then(setIstantanee).catch(onErrore);
  }, [id, onErrore]);

  useEffect(rileggiPacchetto, [rileggiPacchetto]);

  /**
   * La validazione, con un respiro.
   *
   * Centoventi millisecondi: sotto, ogni carattere digitato è un giro completo
   * di parse più compile; sopra, l'anteprima resta indietro rispetto alle dita.
   * `compile_skin` è deterministico e veloce — l'esito dice quanti millisecondi
   * ci ha messo — ma il canale IPC no.
   */
  useEffect(() => {
    if (sorgente.length === 0) return;
    let annullato = false;
    const attesa = setTimeout(() => {
      ipc
        .studioValida(sorgente)
        .then((v) => {
          if (annullato) return;
          setEsito(v);
          // L'anteprima resta all'ultimo stato **valido**: un editor che diventa
          // bianco a metà di una parentesi costringe a scrivere in fretta per
          // paura di romperlo.
          if (v.errori.length === 0) setUltimoValido(v);
        })
        .catch(onErrore);
    }, 120);
    return () => {
      annullato = true;
      clearTimeout(attesa);
    };
  }, [sorgente, onErrore]);

  // La bozza si salva da sé, e si salva **anche se non è valida**: un documento
  // a metà è un lavoro in corso, e perderlo chiudendo la finestra sarebbe il
  // modo peggiore di insegnare a salvare spesso.
  //
  // Quel che è finito su disco si segna, e non è un dettaglio contabile: senza,
  // «non salvata» resta acceso per sempre — anche un minuto dopo che la bozza è
  // stata scritta — e un indicatore che dice sempre la stessa cosa smette di
  // essere un indicatore.
  useEffect(() => {
    if (sorgente.length === 0 || sorgente === originale) return;
    const attesa = setTimeout(() => {
      ipc
        .studioSalva(id, sorgente)
        .then(() => setSalvataAl(sorgente))
        .catch(onErrore);
    }, 800);
    return () => clearTimeout(attesa);
  }, [sorgente, originale, id, onErrore]);

  /**
   * La sonda accende il puntatore, e il tasto `I` accende la sonda.
   *
   * Il gancio globale di `tastiera.ts` è quello del lettore e qui non arriva:
   * lo Studio occupa tutta la finestra ma non è una schermata del lettore. La
   * guardia sul campo di testo è la stessa — senza, scrivere una `i` dentro
   * l'editor spegnerebbe la sonda.
   */
  useEffect(() => {
    const ascolta = (e: KeyboardEvent) => {
      const dove = document.activeElement;
      const scrivendo =
        dove instanceof HTMLInputElement ||
        dove instanceof HTMLTextAreaElement ||
        dove instanceof HTMLSelectElement ||
        (dove instanceof HTMLElement && dove.isContentEditable);

      // Annulla e ripeti **anche** dentro l'editor, e questa è la differenza
      // con la sonda. Di solito si lascia al campo il suo annullo nativo; qui
      // quell'annullo non esiste — la sorgente è controllata e ogni controllo
      // la riscrive da fuori, il che svuota la pila del browser. Intercettarlo
      // non toglie niente a nessuno: restituisce l'unico annullo che c'è.
      if ((e.ctrlKey || e.metaKey) && !e.altKey) {
        const tasto = e.key.toLowerCase();
        if (tasto === "z" && !e.shiftKey) {
          e.preventDefault();
          annulla();
          return;
        }
        if ((tasto === "z" && e.shiftKey) || tasto === "y") {
          e.preventDefault();
          ripeti();
          return;
        }
      }

      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key !== "i" && e.key !== "I") return;
      if (scrivendo) return;
      e.preventDefault();
      setSonda((prima) => !prima);
    };
    window.addEventListener("keydown", ascolta);
    return () => window.removeEventListener("keydown", ascolta);
  }, [annulla, ripeti]);

  const parti = useMemo(
    () => new Map((registro?.parts ?? []).map((p) => [p.name, p])),
    [registro],
  );

  const documento = useMemo(() => leggi(sorgente), [sorgente]);
  const ridisegnate = useMemo(
    () => new Set(Object.keys((documento?.parts ?? {}) as Record<string, unknown>)),
    [documento],
  );
  const dichiarati = useMemo(
    () => new Set(Object.keys((documento?.tokens ?? {}) as Record<string, unknown>)),
    [documento],
  );

  /** Il valore attuale di una proprietà della parte scelta. */
  const valoreDi = useCallback(
    (campo: string): unknown => {
      if (parteScelta === null || documento === null) return undefined;
      return valoreIn(documento, percorsoParte(parteScelta, stato, campo));
    },
    [documento, parteScelta, stato],
  );

  /**
   * Il ponte fra i controlli dell'ispettore e il testo.
   *
   * `null` toglie, e **così anche** un contenitore vuoto: è qui che passa ogni
   * controllo, quindi è qui che conviene tenere l'invariante «nel documento non
   * finisce una dichiarazione che non dichiara niente». Metterla nei singoli
   * controlli vorrebbe dire ricordarsene otto volte, e nove al prossimo.
   */
  const scrivi = useCallback(
    (campo: string, valore: unknown) => {
      if (parteScelta === null) return;
      const dove = percorsoParte(parteScelta, stato, campo);
      setSorgente((prima) =>
        valore === null || vuoto(valore)
          ? togliDa(prima, dove)
          : scriviIn(prima, dove, valore),
      );
    },
    [parteScelta, stato],
  );

  /**
   * Come `valoreDi` e `scrivi`, ma sempre alla base della parte.
   *
   * Serve a `layer`, che è l'unica dichiarazione di una parte a **non** essere
   * un aspetto: `document.rs` la toglie dalla mappa prima di validare il resto
   * (`solo_aspetto.remove("layer")`), quindi esiste solo in
   * `parts.<nome>.layer` e non ha una versione per stato. Passarla dal funnel
   * normale scriverebbe `parts.x.states.hover.layer`, che il parser rifiuta —
   * un errore che l'interfaccia avrebbe suggerito.
   */
  const valoreBase = useCallback(
    (campo: string): unknown => {
      if (parteScelta === null || documento === null) return undefined;
      return valoreIn(documento, ["parts", parteScelta, campo]);
    },
    [documento, parteScelta],
  );

  const scriviBase = useCallback(
    (campo: string, valore: unknown) => {
      if (parteScelta === null) return;
      const dove = ["parts", parteScelta, campo];
      setSorgente((prima) =>
        valore === null || vuoto(valore)
          ? togliDa(prima, dove)
          : scriviIn(prima, dove, valore),
      );
    },
    [parteScelta],
  );

  /**
   * Il ponte fra l'editor dei token e il testo.
   *
   * Il prefisso è `["tokens"]` per il tema scuro e `["themes", "light"]` per il
   * chiaro: sono due insiemi dello stesso vocabolario, e passarli come argomento
   * invece di scrivere due funzioni tiene la regola in un posto solo — «`null`
   * toglie, e togliere l'ultimo token di `themes.light` toglie anche
   * `themes.light`», che è `togliDa` a farlo.
   */
  const scriviToken = useCallback(
    (prefisso: readonly string[], valore: unknown) => {
      if (tokenScelto === null) return;
      const dove = [...prefisso, tokenScelto];
      setSorgente((prima) =>
        valore === null || vuoto(valore)
          ? togliDa(prima, dove)
          : scriviIn(prima, dove, valore),
      );
    },
    [tokenScelto],
  );

  /**
   * Un preset: fino a diciotto scritture, **un** passo di annullo.
   *
   * # Perché una `setSorgente` sola, e non una per voce
   *
   * Perché l'accorpamento della storia è **a tempo** (`storia.ts`, «l'accorpamento
   * è a tempo, e non per provenienza»), e a tempo è la regola giusta per un
   * trascinamento — trenta scritture in mezzo secondo sono un gesto — ma è una
   * garanzia statistica, non una promessa. Una `setSorgente` per voce, in fila,
   * cadrebbe quasi sempre dentro la stessa pausa e ogni tanto no: un
   * `JSON.parse` più lungo del solito su un manifest grosso, un fotogramma
   * perso, una macchina carica, e «annulla» tornerebbe a metà preset — cioè a
   * una scena che nessuno ha mai scelto e che non è né quella di prima né quella
   * di dopo. Un preset è un gesto per costruzione, e va scritto come tale invece
   * che sperare che il cronometro lo riconosca.
   *
   * # Perché `scriviIn` in una piega, e non un secondo scrittore
   *
   * Perché è **la stessa** funzione che usa il cursore qui sopra: quel che un
   * preset può scrivere è esattamente quel che un controllo può scrivere, che è
   * la promessa che questo Studio fa da tre revisioni — la stessa che vale per
   * le proposte del modello in `patch.ts`. Il prezzo è che il testo si attraversa
   * una volta per voce; è un clic, non un trascinamento, e la chiarezza vale più
   * dei giri risparmiati.
   *
   * # Perché un frammento che non si legge si salta
   *
   * Il valore arriva dal crate come **testo**, ed è il testo che finisce nel
   * documento: è la ragione per cui la tabella dei preset sta in Rust, dove
   * `ogni_preset_applicato_a_plain_da_una_skin_valida_e_senza_avvisi` la fa
   * passare dalle guardie vere. Quindi un frammento illeggibile non è un caso da
   * gestire, è un crate cambiato sotto i piedi — e allora le voci rimaste sono
   * comunque quel che l'autore ha chiesto. Fermare tutto per la prima
   * riga storta lascerebbe una skin invariata e nessuna spiegazione; scrivere
   * le altre lascia una skin coerente e una riga in console.
   */
  const applicaPreset = useCallback((preset: PresetRegistro) => {
    setSorgente((prima) =>
      preset.valori.reduce((testo, [token, frammento]) => {
        let valore: unknown;
        try {
          valore = JSON.parse(frammento);
        } catch (e: unknown) {
          console.error(`preset «${preset.id}», token «${token}»:`, e);
          return testo;
        }
        return scriviIn(testo, ["tokens", token], valore);
      }, prima),
    );
  }, []);

  /** La skin promette un tema chiaro: solo allora la seconda colonna serve. */
  const chiaroPromesso =
    valoreIn(documento ?? {}, ["capabilities", "light"]) === true;

  /** I motivi dichiarati: nome → effetto per esteso. */
  const motivi = useMemo(
    () => (documento?.["patterns"] ?? {}) as Readonly<Record<string, unknown>>,
    [documento],
  );

  /**
   * Mette il lavoro nell'elenco delle skin, e lo indossa.
   *
   * # Perché non basta l'esportazione
   *
   * «Esporta .aeskin» serve a **dare** un tema a qualcun altro: chiede dove
   * mettere il file, e il file da solo non cambia niente qui dentro. Per usare
   * il proprio lavoro bisognava esportarlo in una cartella qualunque e
   * reinstallarlo dalla finestra di dialogo di Impostazioni — due scelte di
   * percorso per un file che nessuno voleva davvero.
   *
   * La bozza si salva prima: se l'installazione fallisce, il lavoro è comunque
   * su disco.
   */
  const salvaEUsa = async () => {
    try {
      await ipc.studioSalva(id, sorgente);
      // Un'istantanea prima di ogni prova: è il momento in cui si sta per far
      // diventare vero quel che finora era una bozza, ed è esattamente il punto
      // a cui si vorrà tornare se la prova non piace.
      await ipc.studioIstantanea(id, sorgente, "salvata");
      const voce = await ipc.skinInstallaSorgente(sorgente);
      // La pillola «bozza» si spegne: da adesso il disco dice quel che dice
      // l'editor.
      setOriginale(sorgente);
      rileggiPacchetto();
      onInstallata(voce.id);
    } catch (e) {
      onErrore(e);
    }
  };

  const esporta = async () => {
    const dove = await save({
      defaultPath: `${String(documento?.["id"] ?? id)}.aeskin`,
      filters: [{ name: "Skin di Aether", extensions: ["aeskin"] }],
    });
    if (typeof dove !== "string") return;
    try {
      await ipc.studioIstantanea(id, sorgente, "esportata");
      await ipc.studioEsporta(id, sorgente, dove);
      rileggiPacchetto();
    } catch (e) {
      onErrore(e);
    }
  };

  /**
   * Butta la bozza e torna a quel che dice il pacchetto.
   *
   * Un'istantanea **prima**: è l'unica azione irreversibile dello Studio, e
   * l'unica rete che ha senso stendere sotto è quella che c'è già. Chi si pente
   * la ritrova in cima all'elenco.
   */
  const scartaBozza = async () => {
    try {
      await ipc.studioIstantanea(id, sorgente, "manuale");
      const dalPacchetto = await ipc.studioScarta(id);
      // Non `riparti`: buttare la bozza è una modifica come le altre, e
      // annullarla deve essere possibile finché la finestra è aperta.
      setSorgente(dalPacchetto);
      setOriginale(dalPacchetto);
      setSalvataAl(dalPacchetto);
      rileggiPacchetto();
    } catch (e) {
      onErrore(e);
    }
  };

  /**
   * Riscrive il documento nella forma di `plain.json`: due spazi, e le chiavi
   * dov'erano. Spento quando il testo non è JSON — riformattare quel che non si
   * riesce a leggere vorrebbe dire riscriverci sopra.
   */
  const formatta = () => {
    if (documento === null) return;
    setSorgente(riscrivi(documento));
  };

  /**
   * Porta il fuoco dove sta il problema.
   *
   * Prima cambiava vista e buttava via il percorso, che è la metà di quel che un
   * bottone «vai» promette: si finiva sul documento giusto e poi bisognava
   * cercare la riga a mano. La riga adesso la dice il nucleo, ed è la stessa
   * che sottolinea l'errore — così il bottone e la sottolineatura non possono
   * indicare due punti diversi.
   */
  const vaiA = (percorso: string) => {
    setVista("documento");
    setScheda("json");
    setVaiAlla(percorso);
  };

  /**
   * L'errore di sintassi, quando è quello a tenere fermo il documento.
   *
   * Uno solo per costruzione: se il testo non è JSON, `leggi_skin` si ferma lì
   * e non ha nessuno schema da controllare. Si riconosce dal fatto che porta una
   * riga senza portare un percorso — un problema di schema ha tutti e due.
   */
  const sintassi =
    esito?.errori.length === 1 &&
    esito.errori[0] !== undefined &&
    esito.errori[0].path === "" &&
    esito.errori[0].riga !== null
      ? esito.errori[0]
      : null;

  /** Porta il cursore dove il documento si è rotto. */
  const vaiAllaRottura = () => {
    setVista("documento");
    setScheda("json");
    if (sintassi !== null) setVaiAllaRiga(sintassi.riga);
  };

  const errori = esito?.errori.length ?? 0;
  const avvisi = esito?.avvisi.length ?? 0;
  const nome = String(
    ((documento?.meta ?? {}) as Record<string, unknown>)["name"] ?? id,
  );
  const versione = String(
    ((documento?.meta ?? {}) as Record<string, unknown>)["version"] ?? "—",
  );
  /**
   * Il documento porta ancora l'identificatore della skin di serie.
   *
   * `plain` viene dal binario e non dalla cartella, quindi installarlo non
   * significa niente e il nucleo lo rifiuta (`skin.rs`). Aprire lo Studio su
   * «Plain» è però il modo legittimo di guardare com'è fatta la skin di
   * riferimento — e ora anche di derivarne una, cambiando l'id qui dentro.
   */
  const diSerie = documento?.["id"] === "plain";
  const capacita = (documento?.["capabilities"] ?? {}) as Record<string, boolean>;
  const tavolozza = (documento?.["palette"] ?? {}) as Record<string, string>;
  /** Quante superfici sforano il budget, e quante coppie non si leggono. */
  const fuoriBudget = (esito?.avvisi ?? []).filter((a) => a.kind === "costBudget").length;
  const sottoSoglia = (esito?.contrasti ?? []).filter((c) => !c.passa).length;

  /** Le voci dell'albero di sinistra, filtrate. */
  const voci = useMemo(() => {
    const q = filtro.trim().toLowerCase();
    if (lato === "parti") {
      const per = new Map<string, typeof registro extends null ? never : NonNullable<typeof registro>["parts"]>();
      for (const p of registro?.parts ?? []) {
        if (q.length > 0 && !p.name.includes(q) && !p.description.toLowerCase().includes(q)) {
          continue;
        }
        const dentro = per.get(p.group) ?? [];
        dentro.push(p);
        per.set(p.group, dentro);
      }
      return [...per.entries()];
    }
    const per = new Map<string, NonNullable<typeof registro>["tokens"]>();
    for (const t of registro?.tokens ?? []) {
      if (q.length > 0 && !t.id.includes(q) && !t.description.toLowerCase().includes(q)) {
        continue;
      }
      const dentro = per.get(t.group) ?? [];
      dentro.push(t);
      per.set(t.group, dentro);
    }
    return [...per.entries()];
  }, [registro, filtro, lato]);

  /**
   * L'impaginazione in prova: l'albero da mostrare e da modificare, e le due
   * manopole che vivono in un attributo invece che nel foglio.
   *
   * Viene dall'esito della validazione, non dal documento: arriva già completa
   * — misure popolate, manopole coi difetti, nomi passati dal registro — quindi
   * l'editor non deve ricomporla dal documento e dalla tabella dei difetti
   * insieme. Come l'anteprima, resta all'ultimo stato **valido** mentre si
   * scrive.
   */
  const impaginazione =
    (esito?.errori.length ?? 0) > 0
      ? (ultimoValido?.layout ?? null)
      : (esito?.layout ?? null);
  const albero = impaginazione?.shell ?? null;

  /**
   * Il foglio della skin in prova, con la stessa regola dell'impaginazione qui
   * sopra: mentre si scrive del JSON rotto si guarda l'ultimo che stava in
   * piedi, perché un'anteprima che sbianca a metà di una parentesi non dice
   * niente a nessuno.
   *
   * Stava scritto due volte, una per ognuna delle due `Anteprima`. Adesso ha un
   * lettore in più — lo slot della pagina dei pannelli, che lo passa alla scena
   * dello spettro — e tre copie della stessa espressione sono tre occasioni di
   * farne divergere una.
   */
  const cssAnteprima =
    (esito?.errori.length ?? 0) > 0
      ? (ultimoValido?.css ?? "")
      : (esito?.css ?? "");

  /**
   * Ogni gesto è una `scriviIn` sola.
   *
   * Strutturale o scalare non fa differenza: si riscrive `layout.shell` intero
   * da un albero clonato. `JSON.stringify` è deterministico e le chiavi escono
   * nell'ordine fisso di `versoDocumento`, quindi uno scatto di cursore produce
   * comunque un diff di una riga — e `patch.ts` non ha bisogno di imparare a
   * indicizzare gli array.
   */
  const scriviAlbero = (nuovo: NodoScafale) => {
    setSorgente((prima) => scriviIn(prima, ["layout", "shell"], versoDocumento(nuovo)));
  };

  /** I prefab dichiarati, letti dal documento: sono nomi, e i nomi stanno lì. */
  const prefabs = Object.keys(
    ((documento?.["layout"] ?? {}) as Record<string, unknown>)["prefabs"] ?? {},
  );

  /**
   * «Fanne un prefab»: solleva il sottoalbero e lascia un riferimento.
   *
   * Due scritture in una battuta sola, e in quest'ordine: prima il corpo dentro
   * `layout.prefabs`, poi lo scafale che lo richiama. Al contrario, per un
   * istante il documento conterrebbe un riferimento a un prefab che non esiste
   * ancora — e la validazione, che gira su ogni battuta, lo direbbe.
   */
  const sollevaPrefab = (via: Via) => {
    if (albero === null) return;
    const nodo = nodoA(albero, via);
    if (nodo === null || nodo.kind !== "zone") return;
    const nome = nomePrefabLibero(nodo, prefabs);
    setSorgente((prima) => {
      const conCorpo = scriviIn(prima, ["layout", "prefabs", nome], corpoPrefab(nodo));
      const riferito = conNodo(albero, via, { ...nodo, fromPrefab: nome });
      return scriviIn(conCorpo, ["layout", "shell"], versoDocumento(riferito));
    });
  };

  /**
   * Le sovrapposizioni accese **e** possibili su questa pagina.
   *
   * Un interruttore lasciato acceso e poi diventato impossibile — si accende la
   * coda, si passa a schermo intero — non arriva al renderer: là produrrebbe uno
   * stato che l'applicazione non ha. Si filtra qui invece che nel gesto, così
   * tornando alla pagina di prima lo si ritrova acceso.
   */
  const attive = useMemo(() => effettive(pagina, accese), [pagina, accese]);

  /** Il contesto e gli slot dell'anteprima: il mondo finto, per una scena. */
  const finto = useMemo(() => contestoFinto(pagina, attive), [pagina, attive]);
  /**
   * Quel che riempie il buco «contenuto» dell'albero.
   *
   * L'albero decide **dove** va la pagina, l'app decide **quale** pagina è: il
   * secondo è instradamento, e una skin non ha titolo a sceglierlo. Ma senza
   * niente là dentro le pagine dell'applicazione non comparivano in nessuna
   * scena — e una parte che non si vede non si può ridipingere. Adesso ogni
   * pagina porta il markup vero di quella schermata.
   */
  const slot = useMemo(
    () => slotDellaPagina(pagina, finto, cssAnteprima),
    [pagina, finto, cssAnteprima],
  );

  /**
   * Accende o spegne una sovrapposizione.
   *
   * Non tocca la pagina: sono due assi, e cambiarne uno per l'altro sarebbe il
   * difetto che questa vista ha appena smesso di avere.
   */
  const commuta = useCallback((quale: Sovrapposizione) => {
    setAccese((prima) => {
      const dopo = new Set(prima);
      if (dopo.has(quale)) dopo.delete(quale);
      else dopo.add(quale);
      return dopo;
    });
  }, []);

  /**
   * Porta dove la parte scelta si vede: la pagina **e** gli interruttori.
   *
   * Prima cambiava solo la scena, che con un asse solo era tutto quel che c'era
   * da cambiare. Adesso «portami dalla barra della selezione» vuol dire una
   * pagina qualunque con un interruttore acceso, e accenderlo è metà del
   * viaggio.
   */
  const portamiA = useCallback((dove: (typeof DOVE_SI_VEDE)[string]) => {
    if (dove.pagina !== undefined) setPagina(dove.pagina);
    if (dove.accendi !== undefined) {
      const quali = dove.accendi;
      setAccese((prima) => new Set([...prima, ...quali]));
    }
  }, []);

  const definizione =
    registro?.parts.find((p) => p.name === parteScelta) ?? null;

  /**
   * Il trigger che corrisponde allo stato scelto nell'ispettore.
   *
   * `base` scrive `enter`, e non è una traduzione arbitraria: `enter` **è** la
   * regola base della parte — la stessa che porta lo sfondo e il bordo — e
   * un'animazione lì parte quando l'elemento entra nel documento. Gli altri
   * quattro hanno lo stesso nome di là e di qua perché `AnimTrigger::State`
   * riusa `PartState` invece di ricopiarlo.
   */
  const trigger = stato ?? "enter";

  /** Le animazioni che il documento dichiara, in ordine di nome. */
  const nomiAnimazioni = useMemo(() => {
    const dichiarate = (documento?.["motion"] ?? {}) as Record<string, unknown>;
    const elenco = dichiarate["animations"];
    return elenco !== null && typeof elenco === "object"
      ? Object.keys(elenco as Record<string, unknown>).sort()
      : [];
  }, [documento]);

  const animazioneScelta =
    parteScelta === null || documento === null
      ? null
      : ((): string | null => {
          const scritta = valoreIn(documento, [
            "parts",
            parteScelta,
            "animations",
            trigger,
          ]);
          return typeof scritta === "string" ? scritta : null;
        })();

  const scriviAnimazione = useCallback(
    (nome: string | null) => {
      if (parteScelta === null) return;
      const dove = ["parts", parteScelta, "animations", trigger];
      setSorgente((prima) =>
        nome === null ? togliDa(prima, dove) : scriviIn(prima, dove, nome),
      );
    },
    [parteScelta, trigger],
  );
  const obbligatori = registro?.tokens.filter((t) => t.required).length ?? 0;
  const mancanti =
    registro?.tokens.filter((t) => t.required && !dichiarati.has(t.id))
      .length ?? 0;

  return (
    <section
      className={chat ? "studio con-chat" : "studio"}
      aria-label={t("studio.title")}
    >
      <header className="testa-studio">
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("studio.leave")}
          onClick={onEsci}
        >
          <Icona nome="i-chev-l" dim={17} />
        </button>
        <span className="marchio-studio">
          <Icona nome="i-skin" dim={18} />
          {t("studio.title")}
        </span>
        <span className="divisore" />
        <span className="pillola-skin">
          <strong>{nome}</strong>
          <code>{versione}</code>
          {sorgente !== originale && (
            <span className="bozza">{t("studio.draft")}</span>
          )}
          {errori > 0 && (
            <span className="rotta">{t("studio.errors", { n: errori })}</span>
          )}
        </span>

        <Segmentato
          etichetta={t("studio.whichView")}
          scelta={vista}
          onScegli={setVista}
          classe="minuto"
          voci={[
            { chiave: "ispeziona", etichetta: t("studio.view.ispeziona") },
            { chiave: "impagina", etichetta: t("studio.view.impagina") },
            { chiave: "documento", etichetta: t("studio.view.documento") },
            { chiave: "tavolozza", etichetta: t("studio.view.tavolozza") },
          ]}
        />

        {/* Annulla e ripeti, accanto al nome e non in un menu: sono i due
            bottoni che si cercano subito dopo aver sbagliato, e cercarli è già
            metà del fastidio. La scorciatoia è quella di sempre. */}
        <span className="coppia-storia">
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("studio.undo")}
            title={t("studio.undo.key")}
            disabled={!puoAnnullare}
            onClick={annulla}
          >
            <Icona nome="i-chev-l" dim={15} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("studio.redo")}
            title={t("studio.redo.key")}
            disabled={!puoRipetere}
            onClick={ripeti}
          >
            <Icona nome="i-chev-r" dim={15} />
          </button>
        </span>

        <span className="spinta" />

        {/* La variante chiara si può guardare solo se la skin dichiara di
            averla: un interruttore che c'è sempre e a volte non fa niente è
            proprio il difetto che la vista Tavolozza denuncia. */}
        {capacita["light"] && (
          <Segmentato
            etichetta={t("studio.whichVariant")}
            scelta={tema}
            onScegli={setTema}
            classe="minuto"
            voci={[
              { chiave: "dark", etichetta: t("studio.variant.dark") },
              { chiave: "light", etichetta: t("studio.variant.light") },
            ]}
          />
        )}

        {vista === "documento" && (
          <button
            type="button"
            className="pillola btn-ghost"
            disabled={documento === null}
            title={documento === null ? t("studio.notJson") : undefined}
            onClick={formatta}
          >
            <Icona nome="i-sort" dim={15} />
            {t("studio.format")}
          </button>
        )}

        {(vista === "ispeziona" || vista === "impagina") && (
          <button
            type="button"
            className="pillola btn-ghost"
            aria-pressed={sonda}
            onClick={() => setSonda((prima) => !prima)}
          >
            <Icona nome="i-search" dim={15} />
            {t("studio.probe")}
            <kbd className="scorciatoia">I</kbd>
          </button>
        )}
        {/* La chat sta prima di «Salva e usa» e non in fondo: è la cosa che si
            apre **mentre** si lavora, e le due in fondo sono le due con cui si
            finisce. */}
        <button
          type="button"
          className="pillola btn-ghost"
          aria-pressed={chat}
          onClick={() => setChat((prima) => !prima)}
        >
          <Icona nome="i-chat" dim={15} />
          {t("studio.chat.open")}
        </button>
        <button
          type="button"
          className="pillola btn-accent"
          disabled={errori > 0 || diSerie}
          title={
            diSerie
              ? t("studio.stockSkin")
              : errori > 0
                ? t("studio.errorsBlockInstall")
                : undefined
          }
          onClick={() => void salvaEUsa()}
        >
          <Icona nome="i-check" dim={15} />
          {t("studio.saveAndUse")}
        </button>
        {/* Il consiglio del suggerimento, fatto bottone. Dire «cambia id e
            nome» accanto a un bottone spento lascia comunque da cercare dove:
            il pannello Identità nella vista Tavolozza è quel dove, ed esiste da
            adesso. */}
        {diSerie && (
          <button
            type="button"
            className="pillola btn-ghost"
            title={t("studio.nameIt.why")}
            onClick={() => setVista("tavolozza")}
          >
            <Icona nome="i-mark" dim={15} />
            {t("studio.nameIt")}
          </button>
        )}
        <button
          type="button"
          className="pillola btn-ghost"
          disabled={errori > 0}
          title={errori > 0 ? t("studio.export.blocked") : undefined}
          onClick={() => void esporta()}
        >
          <Icona nome="i-import" dim={15} />
          {t("studio.export.do")}
        </button>
      </header>

      {/*
        Quando il testo non è JSON, i controlli non scrivono.

        Non è una scelta di questa fascia: `scriviIn` restituisce la sorgente
        intatta se non riesce a leggerla, ed è giusto — riscrivere sopra un
        documento a metà di una parentesi vorrebbe dire buttare via quel che si
        stava scrivendo. Il difetto era il silenzio: si trascinava un cursore,
        non succedeva niente, e non c'era modo di sapere perché. Adesso lo dice,
        e offre le due uscite invece di lasciarle cercare.
      */}
      {documento === null && sorgente.length > 0 && (
        <div className="testo-rotto" role="status">
          <Icona nome="i-alert" dim={15} />
          <span>
            {/*
              * Quando il nucleo dice **dove**, lo dice la fascia: «riga 42 ·
              * colonna 7 — manca una virgola fra due campi» invece di «il
              * documento non è JSON valido», che è la stessa frase per tutte le
              * ragioni possibili. Il ripiego resta per l'istante fra la battuta
              * e la validazione, che arriva centoventi millisecondi dopo.
              */}
            {sintassi === null ? (
              <Trans
                k="studio.broken"
                v={{
                  nonScrivono: <strong>{t("studio.broken.controls")}</strong>,
                }}
              />
            ) : (
              sintassi.message
            )}
          </span>
          {sintassi?.riga != null && (
            <span className="dove-rotto">
              {t("studio.doc.rowCol", {
                riga: sintassi.riga,
                colonna: sintassi.colonna ?? 1,
              })}
            </span>
          )}
          <button
            type="button"
            className="pillola btn-ghost"
            onClick={() => vaiAllaRottura()}
          >
            <Icona nome="i-text" dim={14} />
            {t("studio.broken.goText")}
          </button>
          <button
            type="button"
            className="pillola btn-ghost"
            disabled={!puoAnnullare}
            onClick={annulla}
          >
            <Icona nome="i-chev-l" dim={14} />
            {t("studio.undo")}
          </button>
        </div>
      )}

      {vista === "documento" && (
        <Documento
          sorgente={sorgente}
          onSorgente={setSorgente}
          esito={esito}
          ultimoValido={ultimoValido}
          scheda={scheda}
          onScheda={setScheda}
          soglia={registro?.contrastoMinimo ?? 4.5}
          originale={originale}
          registro={registro}
          idSkin={String(documento?.["id"] ?? id)}
          vaiAlla={vaiAlla}
          vaiAllaRiga={vaiAllaRiga}
          onArrivato={() => {
            setVaiAlla(null);
            setVaiAllaRiga(null);
          }}
          onCorreggi={(percorso, giusto) =>
            setSorgente((prima) => rinominaChiave(prima, percorso, giusto))
          }
          colonnaSinistra={
            <Pacchetto
              id={String(documento?.["id"] ?? id)}
              voci={fileDelPacchetto}
              istantanee={istantanee}
              parti={ridisegnate.size}
              token={dichiarati.size}
              // Rispetto alla **bozza su disco**, non alla skin installata:
              // altrimenti restava «non salvata» per sempre, perché il salvataggio
              // automatico non tocca il pacchetto e non poteva farlo scendere.
              sporca={sorgente !== salvataAl}
              derivata={sorgente !== originale}
              onScarta={() => void scartaBozza()}
              onIstantanea={() => {
                ipc
                  .studioIstantanea(id, sorgente, "manuale")
                  .then(rileggiPacchetto)
                  .catch(onErrore);
              }}
              onRipristina={(quando) => {
                ipc
                  .studioRipristina(id, quando)
                  .then(setSorgente)
                  .catch(onErrore);
              }}
            />
          }
        />
      )}

      {vista === "tavolozza" && (
        <Tavolozza
          sorgente={sorgente}
          onSorgente={setSorgente}
          esito={esito}
          registro={registro}
          onEsporta={() => void esporta()}
          onVaiA={vaiA}
          onApriToken={(token) => {
            setTokenScelto(token);
            setLato("token");
            setVista("ispeziona");
          }}
        />
      )}

      {vista === "ispeziona" && (
        <div className="corpo-studio">
          <aside className="registro">
            <input
              className="filtro field-input"
              type="search"
              placeholder={t("studio.registry.search")}
              value={filtro}
              onChange={(e) => setFiltro(e.target.value)}
              spellCheck={false}
            />
            <Segmentato
              etichetta={t("studio.registry.what")}
              scelta={lato}
              onScegli={setLato}
              classe="minuto"
              voci={[
                {
                  chiave: "token",
                  etichetta: t("studio.registry.tokens"),
                  conteggio: registro?.tokens.length,
                },
                {
                  chiave: "parti",
                  etichetta: t("studio.registry.parts"),
                  conteggio: registro?.parts.length,
                },
              ]}
            />

            <div className="albero">
              {voci.map(([gruppo, dentro]) => {
                // Con un filtro attivo si aprono tutti: un albero chiuso su una
                // ricerca è una ricerca senza risultati.
                const aperto =
                  filtro.trim().length > 0 ||
                  aperti.has(gruppo) ||
                  dentro.some((v) =>
                    "name" in v ? v.name === parteScelta : v.id === tokenScelto,
                  );
                return (
                  <div key={gruppo} className="gruppo-registro">
                    <button
                      type="button"
                      className="titolo-gruppo"
                      aria-expanded={aperto}
                      onClick={() =>
                        setAperti((prima) => {
                          const dopo = new Set(prima);
                          if (dopo.has(gruppo)) dopo.delete(gruppo);
                          else dopo.add(gruppo);
                          return dopo;
                        })
                      }
                    >
                      <Icona nome={aperto ? "i-chev-d" : "i-chev-r"} dim={13} />
                      <span>{nomeGruppo(gruppo)}</span>
                      <span className="quante">{dentro.length}</span>
                    </button>
                    {aperto &&
                      dentro.map((voce) => {
                        const nomeVoce = "name" in voce ? voce.name : voce.id;
                        const toccata =
                          "name" in voce
                            ? ridisegnate.has(voce.name)
                            : dichiarati.has(voce.id);
                        return (
                          <button
                            key={nomeVoce}
                            type="button"
                            className="voce-registro"
                            data-active={
                              ("name" in voce
                                ? parteScelta === voce.name
                                : tokenScelto === voce.id) || undefined
                            }
                            title={
                          "name" in voce
                            ? descrizioneParte(voce.name, voce.description)
                            : descrizioneToken(voce.id, voce.description)
                        }
                            // Un token si apre come si apre una parte. Prima questo
                            // ramo restituiva `undefined`: l'albero mostrava tutte
                            // le voci del registro — oggi settantatré — e nessuna
                            // di esse era un bottone che portasse da qualche parte.
                            onClick={() =>
                              "name" in voce
                                ? setParteScelta(voce.name)
                                : setTokenScelto(voce.id)
                            }
                          >
                            {/* Il pallino dice se la skin l'ha già toccata: senza,
                            per sapere cosa si è ridisegnato bisogna leggere il
                            JSON, e a quel punto l'albero non serve. */}
                            <span
                              className="pallino"
                              data-toccata={toccata || undefined}
                            />
                            <code className="nome-voce">{nomeVoce}</code>
                            {"layers" in voce && voce.layers && (
                              <Icona
                                nome="i-list"
                                dim={11}
                                titolo={t("studio.registry.freeLayer")}
                              />
                            )}
                            {"required" in voce && voce.required && (
                              <span className="obbligatorio">•</span>
                            )}
                          </button>
                        );
                      })}
                  </div>
                );
              })}
            </div>

            <footer className="piede-registro">
              <span className="titolino">
                {lato === "parti"
                  ? t("studio.registry.partsByGroup")
                  : t("studio.registry.tokensByGroup")}
              </span>
              {/* I chip rispondono «quanto è grande il vocabolario, e dov'è»
                  senza aprire i gruppi: è la domanda che si fa una volta sola,
                  ma all'inizio. */}
              <div className="chip-gruppi">
                {voci.map(([gruppo, dentro]) => (
                  <button
                    key={gruppo}
                    type="button"
                    className="chip-gruppo"
                    data-active={aperti.has(gruppo) || undefined}
                    onClick={() =>
                      setAperti((prima) => {
                        const dopo = new Set(prima);
                        if (dopo.has(gruppo)) dopo.delete(gruppo);
                        else dopo.add(gruppo);
                        return dopo;
                      })
                    }
                  >
                    {nomeGruppo(gruppo)}
                    <span className="quante">{dentro.length}</span>
                  </button>
                ))}
              </div>
              <div className="sintesi">
                {lato === "parti" ? (
                  t("studio.registry.redrawn", {
                    n: ridisegnate.size,
                    quante: registro?.parts.length ?? 0,
                  })
                ) : (
                  <>
                    {t("studio.registry.required", { n: obbligatori })}
                    {mancanti > 0 && (
                      <>
                        {" · "}
                        <span className="manca">
                          {t("studio.registry.undeclared", { n: mancanti })}
                        </span>
                      </>
                    )}
                  </>
                )}
              </div>
            </footer>
          </aside>

          <div className="centro-studio">
            <TestaScene
              larghezza={larghezza}
              pagina={pagina}
              accese={accese}
              onPagina={setPagina}
              onCommuta={commuta}
            >
              <Segmentato
                etichetta={t("studio.whichScale")}
                scelta={String(ingrandimento)}
                onScegli={(v) =>
                  setIngrandimento(v === "null" ? null : Number(v))
                }
                classe="minuto denso"
                voci={ingrandimenti().map(([etichetta, quanto]) => ({
                  chiave: String(quanto),
                  etichetta,
                }))}
              />
              <span className="misura-anteprima">
                {MISURA.larghezza}×{MISURA.altezza} ·{" "}
                {ingrandimento === null
                  ? `${scala}%`
                  : `${ingrandimento * 100}%`}
              </span>
            </TestaScene>
            <Anteprima
              css={cssAnteprima}
              id={String(documento?.["id"] ?? id)}
              parti={parti}
              sondaAccesa={sonda}
              scelta={parteScelta}
              tema={tema === "light" ? "light" : undefined}
              densita={impaginazione?.density}
              movimento={impaginazione?.motion}
              ingrandimento={ingrandimento}
              onScegli={(parte) => {
                setParteScelta(parte);
                // Scegliere dall'anteprima porta con sé il lato giusto: senza,
                // si clicca una superficie e il pannello resta sul token che si
                // stava guardando prima.
                setLato("parti");
              }}
              onQuante={setQuanteVolte}
              onRigioca={() => setRigiocata((quante) => quante + 1)}
              onLarghezza={setLarghezza}
            >
              {/* L'applicazione, e accanto — non dentro il buco del contenuto
                  — le sovrapposizioni: nell'app stanno fuori dall'albero perché
                  si sovrappongono per definizione, e ficcarle nel contenuto
                  voleva dire mostrarle in un posto in cui non compaiono mai. */}
              <Impaginazione
                key={rigiocata}
                albero={albero}
                contesto={finto}
                slot={slot}
              />
              <Sovrapposte accese={attive} />
            </Anteprima>

            {/*
              «Questa parte qui non si vede», e le tre ragioni per cui.

              Prima erano due, e la seconda copriva un caso che non c'entrava:
              chi sceglieva `tour-tooltip` — che il registro dichiara e nessuna
              schermata disegna — si sentiva rispondere «sta nella cornice,
              comparirà quando l'albero la monta», cioè un'attesa che non finisce
              mai. Le tre risposte vere sono: sta in un'altra scena e ti ci
              porto; sta nella cornice e dipende dall'albero; non la disegna
              ancora nessuno, e nessuna scena te la può mostrare.
            */}
            {parteScelta !== null && lato === "parti" && quanteVolte === 0 && (
              <p
                className="non-si-vede section-card"
                data-mai={NON_ANCORA[parteScelta] !== undefined || undefined}
              >
                <Icona
                  nome={
                    NON_ANCORA[parteScelta] !== undefined ? "i-alert" : "i-search"
                  }
                  dim={13}
                />
                <span>
                  <Trans
                    k={
                      NON_ANCORA[parteScelta] !== undefined
                        ? "studio.notYet"
                        : "studio.notHere"
                    }
                    v={{ parte: <code>.{parteScelta}</code> }}
                  />
                </span>
                {NON_ANCORA[parteScelta] !== undefined ? (
                  <span className="da-dove">{perchePartMai(parteScelta)}</span>
                ) : DOVE_SI_VEDE[parteScelta] !== undefined ? (
                  <button
                    type="button"
                    className="pillola btn-ghost"
                    onClick={() => {
                      const dove = DOVE_SI_VEDE[parteScelta];
                      if (dove !== undefined) portamiA(dove);
                    }}
                  >
                    {t("studio.takeMeThere")}
                  </button>
                ) : (
                  <span className="da-dove">{t("studio.inTheFrame")}</span>
                )}
              </p>
            )}

            {(esito?.errori.length ?? 0) > 0 && (
              <p className="in-pausa">
                <Icona nome="i-alert" dim={13} />
                {t("studio.pausedOnError")}
              </p>
            )}
            {sonda && (
              <p className="spiega-sonda section-card">
                <Trans
                  k="studio.probe.note"
                  v={{
                    titolo: <strong>{t("studio.probe.note.title")}</strong>,
                    non: <strong>{t("studio.probe.note.not")}</strong>,
                  }}
                />
              </p>
            )}
          </div>

          {/* Il pannello segue il lato dell'albero: a sinistra si sceglie fra
              token e parti, e il pannello mostra l'editor di quel che si è
              scelto. Due pannelli affiancati vorrebbero dire che uno dei due è
              sempre vuoto. */}
          {lato === "token" ? (
            <Token
              definizione={
                registro?.tokens.find((t) => t.id === tokenScelto) ?? null
              }
              valore={
                tokenScelto === null || documento === null
                  ? undefined
                  : valoreIn(documento, ["tokens", tokenScelto])
              }
              valoreChiaro={
                tokenScelto === null || documento === null
                  ? undefined
                  : valoreIn(documento, ["themes", "light", tokenScelto])
              }
              chiaroPromesso={chiaroPromesso}
              tokens={registro?.tokens ?? []}
              /* Tutti, non quelli del gruppo giusto: la scelta la fa `Token`
                 confrontando `preset.group` col gruppo del token aperto, ed è
                 lì che deve stare — filtrarli qui vorrebbe dire che questo file
                 sa quale gruppo si sta guardando, cioè la conoscenza che quel
                 componente esiste per non avere. */
              presets={registro?.presets ?? []}
              tavolozza={tavolozza}
              onPreset={applicaPreset}
              onScrivi={(v) => scriviToken(["tokens"], v)}
              onScriviChiaro={(v) => scriviToken(["themes", "light"], v)}
              onVaiAlJson={vaiA}
            />
          ) : (
            <Ispettore
              definizione={definizione}
              stato={stato}
              onStato={setStato}
              valoreDi={valoreDi}
              scrivi={scrivi}
              valoreBase={valoreBase}
              scriviBase={scriviBase}
              tokens={registro?.tokens ?? []}
              effetti={registro?.effects ?? []}
              tavolozza={tavolozza}
              motivi={motivi}
              budget={registro?.budget ?? 10}
              animazioni={nomiAnimazioni}
              animazione={animazioneScelta}
              onAnimazione={scriviAnimazione}
            />
          )}
        </div>
      )}

      {vista === "impagina" && (
        <div className="corpo-studio">
          <Scafale
            albero={albero}
            registro={registro}
            scelto={nodoScelto}
            prefabs={prefabs}
            onScegli={setNodoScelto}
            onAlbero={scriviAlbero}
            onPrefab={sollevaPrefab}
          />

          <div className="centro-studio">
            <TestaScene
              larghezza={larghezza}
              pagina={pagina}
              accese={accese}
              onPagina={setPagina}
              onCommuta={commuta}
            >
              {/* La seconda briciola di pane: `riga › colonna › lettore`. Sono
                  gli indici del percorso, cioè esattamente il percorso JSON. */}
              <span className="misura-anteprima">
                {nodoSotto === null
                  ? `${MISURA.larghezza}×${MISURA.altezza} · ${scala}%`
                  : t("studio.node", { via: nodoSotto })}
              </span>
            </TestaScene>
            <Anteprima
              css={cssAnteprima}
              id={String(documento?.["id"] ?? id)}
              parti={parti}
              sondaAccesa={sonda}
              scelta={parteScelta}
              tema={tema === "light" ? "light" : undefined}
              densita={impaginazione?.density}
              movimento={impaginazione?.motion}
              onScegli={setParteScelta}
              onNodo={setNodoSotto}
              onRigioca={() => setRigiocata((quante) => quante + 1)}
              onLarghezza={setLarghezza}
            >
              {/* L'applicazione, e accanto — non dentro il buco del contenuto
                  — le sovrapposizioni: nell'app stanno fuori dall'albero perché
                  si sovrappongono per definizione, e ficcarle nel contenuto
                  voleva dire mostrarle in un posto in cui non compaiono mai. */}
              <Impaginazione
                key={rigiocata}
                albero={albero}
                contesto={finto}
                slot={slot}
              />
              <Sovrapposte accese={attive} />
            </Anteprima>
            {documento !== null && documento["layout"] === undefined && (
              <p className="spiega-sonda section-card">
                <Trans
                  k="studio.noLayout"
                  v={{ titolo: <strong>{t("studio.noLayout.title")}</strong> }}
                />
              </p>
            )}
          </div>

          <IspettoreNodo
            albero={albero}
            via={nodoScelto}
            registro={registro}
            onAlbero={scriviAlbero}
          />
        </div>
      )}

      {/* Quattro numeri e non tre: gli errori dicono se si esporta, gli altri
          tre dicono cosa si sta per esportare. «Fuori budget» e «sotto 4,5:1»
          erano già calcolati e si vedevano solo entrando in un'altra vista. */}
      {/*
        Fuori dai quattro rami di `vista`, o andrebbe scritto quattro volte; e
        fuori dalla griglia di `.studio`, che dichiara tre righe e ha già un
        quarto figlio che entra ed esce — la fascia `.testo-rotto`. Un quinto
        figlio è il modo di scoprirlo nel posto sbagliato. È quindi un `<aside>`
        fisso, e `.studio.con-chat` guadagna un `padding-right` pari alla sua
        larghezza: nessuna riga si sposta, e tutte e quattro le viste si
        restringono da sole.

        Il `padding` è la differenza che conta rispetto al pannello della coda,
        che galleggia sopra il contenuto di proposito. Qui sotto c'è
        l'anteprima, cioè esattamente la cosa che si sta giudicando: coprirla a
        metà renderebbe inutile la risposta che ci si legge dentro.
      */}
      {chat && (
        <Chat
          sorgente={sorgente}
          onSorgente={setSorgente}
          registro={registro}
          esito={esito}
          onIstantanea={() => {
            void ipc.studioIstantanea(id, sorgente, "manuale").catch(onErrore);
          }}
          onErrore={onErrore}
          onChiudi={() => setChat(false)}
          onVaiAImpostazioni={onModelli}
        />
      )}

      <footer className="piede-studio">
        <span className="voce-piede" data-esito={errori > 0 ? "male" : "bene"}>
          <Icona nome={errori > 0 ? "i-alert" : "i-check"} dim={14} />
          {t("studio.errors", { n: errori })}
        </span>
        <span
          className="voce-piede"
          data-esito={avvisi > 0 ? "attenzione" : undefined}
        >
          <Icona nome={avvisi > 0 ? "i-alert" : "i-check"} dim={14} />
          {t("studio.warnings", { n: avvisi })}
        </span>
        {fuoriBudget > 0 && (
          <span className="voce-piede" data-esito="male">
            <Icona nome="i-alert" dim={14} />
            {t("studio.foot.overBudget", { n: fuoriBudget })}
          </span>
        )}
        {sottoSoglia > 0 && (
          <span className="voce-piede" data-esito="male">
            <Icona nome="i-alert" dim={14} />
            {t("studio.foot.underContrast", {
              n: sottoSoglia,
              soglia: registro?.contrastoMinimo ?? 4.5,
            })}
          </span>
        )}
        <span className="spinta" />
        <span className="mono">
          {t("studio.foot.summary", {
            formato: registro?.format ?? 1,
            parti: esito?.parti ?? 0,
            ms: esito?.compilatoMs ?? 0,
          })}
        </span>
      </footer>
    </section>
  );
}
