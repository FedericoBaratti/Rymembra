/**
 * Lo Skin Studio.
 *
 * # La tesi
 *
 * Una skin è un documento di dati validato. `parse_skin` lo legge, `check_skin`
 * avvisa, `compile_skin` lo trasforma in CSS deterministico. Tutto quel che
 * serve a un editor **esiste già nel crate**: quarantasette token con la loro
 * descrizione e il flag di obbligatorietà, cinquantuno parti coi loro gruppi,
 * undici effetti col costo dichiarato, un vocabolario chiuso per parte, e
 * `nearest_parts()` per i refusi.
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
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  ipc,
  type Istantanea,
  type NodoScafale,
  type Registro,
  type Validazione,
  type VoceFile,
} from "../ipc";
import { Impaginazione } from "../Impaginazione";
import { Icona } from "../parti/Icone";
import { Intestazione } from "../parti/Intestazione";
import { Segmentato } from "../parti/Segmentato";
import { Anteprima, MISURA } from "./Anteprima";
import { Documento, type Scheda } from "./Documento";
import { Ispettore } from "./Ispettore";
import { IspettoreNodo, Scafale } from "./Scafale";
import { Pacchetto } from "./Pacchetto";
import { Tavolozza } from "./Tavolozza";
import { SCENE, contestoFinto, type Scena } from "./finto";
import {
  leggi,
  percorsoParte,
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

/** Le quattro viste dello Studio. */
type Vista = "ispeziona" | "impagina" | "documento" | "tavolozza";

/**
 * I due buchi dell'anteprima.
 *
 * Non sono nell'albero, e non è un ripiego: **l'albero decide dove va la pagina,
 * l'app decide quale pagina è**. Quale vista si stia guardando è instradamento —
 * stato dell'app — e una skin non ha titolo a sceglierlo. Qui dentro va quindi
 * un segnaposto: l'intestazione vera con dei dati inventati, e un riquadro che
 * porta i nomi delle parti che una pagina usa davvero, così la sonda li
 * riconosce come li riconoscerebbe nell'applicazione.
 */
const SLOT_FINTI = {
  intestazione: (
    <Intestazione
      occhiello="Libreria"
      titolo="Album"
      sottotitolo="135 in libreria"
    />
  ),
  contenuto: (
    <div className="dentro">
      <div className="griglia track-grid">
        {[0, 1, 2, 3, 4, 5].map((i) => (
          <div key={i} className="scheda section-card">
            <div className="copertina skeleton" />
          </div>
        ))}
      </div>
    </div>
  ),
} as const;

export function Studio({
  id,
  onEsci,
  onInstallata,
  onErrore,
}: {
  /** La skin da aprire: una bozza, una installata, o quella di serie. */
  id: string;
  onEsci: () => void;
  /** La skin è stata installata da qui: la finestra intera va ridipinta. */
  onInstallata: (id: string) => void;
  onErrore: (e: unknown) => void;
}) {
  const [registro, setRegistro] = useState<Registro | null>(null);
  const [sorgente, setSorgente] = useState("");
  const [originale, setOriginale] = useState("");
  const [esito, setEsito] = useState<Validazione | null>(null);
  const [ultimoValido, setUltimoValido] = useState<Validazione | null>(null);
  const [vista, setVista] = useState<Vista>("ispeziona");
  const [scheda, setScheda] = useState<Scheda>("json");
  const [scena, setScena] = useState<Scena>("libreria");
  const [sonda, setSonda] = useState(true);
  const [parteScelta, setParteScelta] = useState<string | null>(null);
  const [stato, setStato] = useState<string | null>(null);
  const [filtro, setFiltro] = useState("");
  const [lato, setLato] = useState<"token" | "parti">("parti");
  /** Quale variante mostra l'anteprima. Cambia il riquadro, non la finestra. */
  const [tema, setTema] = useState<"dark" | "light">("dark");
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
  /** Il nodo scelto nella vista Impagina, come percorso di indici. */
  const [nodoScelto, setNodoScelto] = useState<Via | null>(null);
  /** L'indirizzo del nodo sotto il puntatore: la seconda briciola di pane. */
  const [nodoSotto, setNodoSotto] = useState<string | null>(null);

  /** La stessa misura, letta come percentuale della finestra vera. */
  const scala = Math.round((larghezza / MISURA.larghezza) * 100);

  useEffect(() => {
    ipc.studioRegistro().then(setRegistro).catch(onErrore);
  }, [onErrore]);

  useEffect(() => {
    ipc
      .studioDocumento(id)
      .then((testo) => {
        setSorgente(testo);
        setOriginale(testo);
      })
      .catch(onErrore);
  }, [id, onErrore]);

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
  useEffect(() => {
    if (sorgente.length === 0 || sorgente === originale) return;
    const attesa = setTimeout(() => {
      ipc.studioSalva(id, sorgente).catch(onErrore);
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
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key !== "i" && e.key !== "I") return;
      const dove = document.activeElement;
      const scrivendo =
        dove instanceof HTMLInputElement ||
        dove instanceof HTMLTextAreaElement ||
        dove instanceof HTMLSelectElement ||
        (dove instanceof HTMLElement && dove.isContentEditable);
      if (scrivendo) return;
      e.preventDefault();
      setSonda((prima) => !prima);
    };
    window.addEventListener("keydown", ascolta);
    return () => window.removeEventListener("keydown", ascolta);
  }, []);

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
   * cercare la riga a mano. La riga la sa già `rigaDi()`, che è la stessa
   * funzione che sottolinea l'errore — così il bottone e la sottolineatura non
   * possono indicare due punti diversi.
   */
  const vaiA = (percorso: string) => {
    setVista("documento");
    setScheda("json");
    setVaiAlla(percorso);
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

  /** Il contesto e gli slot dell'anteprima: il mondo finto, per una scena. */
  const finto = useMemo(() => contestoFinto(scena), [scena]);

  const definizione = registro?.parts.find((p) => p.name === parteScelta) ?? null;
  const obbligatori = registro?.tokens.filter((t) => t.required).length ?? 0;
  const mancanti =
    registro?.tokens.filter((t) => t.required && !dichiarati.has(t.id)).length ?? 0;

  return (
    <section className="studio" aria-label="Skin Studio">
      <header className="testa-studio">
        <button
          type="button"
          className="tasto icon-btn"
          aria-label="Esci dallo Studio"
          onClick={onEsci}
        >
          <Icona nome="i-chev-l" dim={17} />
        </button>
        <span className="marchio-studio">
          <Icona nome="i-skin" dim={18} />
          Skin Studio
        </span>
        <span className="divisore" />
        <span className="pillola-skin">
          <strong>{nome}</strong>
          <code>{versione}</code>
          {sorgente !== originale && <span className="bozza">bozza</span>}
          {errori > 0 && <span className="rotta">{errori} errori</span>}
        </span>

        <Segmentato
          etichetta="Vista dello Studio"
          scelta={vista}
          onScegli={setVista}
          classe="minuto"
          voci={[
            { chiave: "ispeziona", etichetta: "Ispeziona" },
            { chiave: "impagina", etichetta: "Impagina" },
            { chiave: "documento", etichetta: "Documento" },
            { chiave: "tavolozza", etichetta: "Tavolozza" },
          ]}
        />

        <span className="spinta" />

        {/* La variante chiara si può guardare solo se la skin dichiara di
            averla: un interruttore che c'è sempre e a volte non fa niente è
            proprio il difetto che la vista Tavolozza denuncia. */}
        {capacita["light"] && (
          <Segmentato
            etichetta="Quale variante mostra l'anteprima"
            scelta={tema}
            onScegli={setTema}
            classe="minuto"
            voci={[
              { chiave: "dark", etichetta: "Scuro" },
              { chiave: "light", etichetta: "Chiaro" },
            ]}
          />
        )}

        {vista === "documento" && (
          <button
            type="button"
            className="pillola btn-ghost"
            disabled={documento === null}
            title={
              documento === null
                ? "Il documento non è JSON: riformattarlo vorrebbe dire riscriverci sopra"
                : undefined
            }
            onClick={formatta}
          >
            <Icona nome="i-sort" dim={15} />
            Formatta
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
            Sonda
            <kbd className="scorciatoia">I</kbd>
          </button>
        )}
        <button
          type="button"
          className="pillola btn-accent"
          disabled={errori > 0 || diSerie}
          title={
            diSerie
              ? "«Plain» è la skin di serie e non si sovrascrive: cambia «id» e «meta.name» nella scheda Documento, e questo diventa un tema tuo"
              : errori > 0
                ? "Gli errori bloccano l'installazione"
                : undefined
          }
          onClick={() => void salvaEUsa()}
        >
          <Icona nome="i-check" dim={15} />
          Salva e usa
        </button>
        <button
          type="button"
          className="pillola btn-ghost"
          disabled={errori > 0}
          title={errori > 0 ? "Gli errori bloccano l'esportazione" : undefined}
          onClick={() => void esporta()}
        >
          <Icona nome="i-import" dim={15} />
          Esporta .aeskin
        </button>
      </header>

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
          onArrivato={() => setVaiAlla(null)}
          onCorreggi={(sbagliato, giusto) =>
            setSorgente((prima) =>
              prima.replace(`"${sbagliato}"`, `"${giusto}"`),
            )
          }
          colonnaSinistra={
            <Pacchetto
              id={String(documento?.["id"] ?? id)}
              voci={fileDelPacchetto}
              istantanee={istantanee}
              parti={ridisegnate.size}
              token={dichiarati.size}
              sporca={sorgente !== originale}
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
        />
      )}

      {vista === "ispeziona" && (
        <div className="corpo-studio">
          <aside className="registro">
            <input
              className="filtro field-input"
              type="search"
              placeholder="Cerca nel registro…"
              value={filtro}
              onChange={(e) => setFiltro(e.target.value)}
              spellCheck={false}
            />
            <Segmentato
              etichetta="Cosa mostrare"
              scelta={lato}
              onScegli={setLato}
              classe="minuto"
              voci={[
                { chiave: "token", etichetta: "Token", conteggio: registro?.tokens.length },
                { chiave: "parti", etichetta: "Parti", conteggio: registro?.parts.length },
              ]}
            />

            <div className="albero">
              {voci.map(([gruppo, dentro]) => {
                // Con un filtro attivo si aprono tutti: un albero chiuso su una
                // ricerca è una ricerca senza risultati.
                const aperto =
                  filtro.trim().length > 0 ||
                  aperti.has(gruppo) ||
                  dentro.some((v) => ("name" in v ? v.name : v.id) === parteScelta);
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
                    <span>{gruppo}</span>
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
                        data-active={parteScelta === nomeVoce || undefined}
                        title={voce.description}
                        onClick={() =>
                          "name" in voce ? setParteScelta(voce.name) : undefined
                        }
                      >
                        {/* Il pallino dice se la skin l'ha già toccata: senza,
                            per sapere cosa si è ridisegnato bisogna leggere il
                            JSON, e a quel punto l'albero non serve. */}
                        <span className="pallino" data-toccata={toccata || undefined} />
                        <code className="nome-voce">{nomeVoce}</code>
                        {"layers" in voce && voce.layers && (
                          <Icona nome="i-list" dim={11} titolo="ha un livello libero" />
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
                {lato === "parti" ? "Parti per gruppo" : "Token per gruppo"}
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
                    {gruppo}
                    <span className="quante">{dentro.length}</span>
                  </button>
                ))}
              </div>
              <div className="sintesi">
                {lato === "parti" ? (
                  `${ridisegnate.size} ridisegnate su ${registro?.parts.length ?? 0}`
                ) : (
                  <>
                    {obbligatori} obbligatori
                    {mancanti > 0 && (
                      <>
                        {" · "}
                        <span className="manca">{mancanti} non dichiarati</span>
                      </>
                    )}
                  </>
                )}
              </div>
            </footer>
          </aside>

          <div className="centro-studio">
            {/* La testata è larga quanto il riquadro, misurato: le linguette
                stanno sopra la scena che cambiano, e la lettura della scala
                finisce sull'angolo di quel che misura. */}
            <div className="testa-centro" style={{ width: larghezza }}>
              <Segmentato
                etichetta="Quale schermata"
                scelta={scena}
                onScegli={setScena}
                classe="minuto"
                voci={SCENE.map(([chiave, etichetta]) => ({ chiave, etichetta }))}
              />
              <span className="spinta" />
              <span className="misura-anteprima">
                {MISURA.larghezza}×{MISURA.altezza} · {scala}%
              </span>
            </div>
            <Anteprima
              css={(esito?.errori.length ?? 0) > 0 ? (ultimoValido?.css ?? "") : (esito?.css ?? "")}
              id={String(documento?.["id"] ?? id)}
              parti={parti}
              sondaAccesa={sonda}
              scelta={parteScelta}
              tema={tema === "light" ? "light" : undefined}
              densita={impaginazione?.density}
              movimento={impaginazione?.motion}
              onScegli={setParteScelta}
              onLarghezza={setLarghezza}
            >
              <Impaginazione albero={albero} contesto={finto} slot={SLOT_FINTI} />
            </Anteprima>
            {(esito?.errori.length ?? 0) > 0 && (
              <p className="in-pausa">
                <Icona nome="i-alert" dim={13} />
                In pausa sull&apos;errore: l&apos;anteprima resta all&apos;ultimo
                stato valido, e non lampeggia mentre scrivi.
              </p>
            )}
            {sonda && (
              <p className="spiega-sonda section-card">
                <strong>Sonda attiva.</strong> Il puntatore illumina la parte più
                interna che la skin può ridisegnare; la briciola di pane mostra le
                superfici sopra. Quel che <strong>non</strong> è una parte non si
                illumina — ed è il modo più rapido per scoprire che una superficie
                non è ancora nel registro.
              </p>
            )}
          </div>

          <Ispettore
            definizione={definizione}
            stato={stato}
            onStato={setStato}
            valoreDi={valoreDi}
            scrivi={scrivi}
            tokens={registro?.tokens ?? []}
            effetti={registro?.effects ?? []}
            tavolozza={tavolozza}
            budget={registro?.budget ?? 10}
          />
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
            <div className="testa-centro" style={{ width: larghezza }}>
              <Segmentato
                etichetta="Quale schermata"
                scelta={scena}
                onScegli={setScena}
                classe="minuto"
                voci={SCENE.map(([chiave, etichetta]) => ({ chiave, etichetta }))}
              />
              <span className="spinta" />
              {/* La seconda briciola di pane: `riga › colonna › lettore`. Sono
                  gli indici del percorso, cioè esattamente il percorso JSON. */}
              <span className="misura-anteprima">
                {nodoSotto === null ? `${MISURA.larghezza}×${MISURA.altezza} · ${scala}%` : `nodo ${nodoSotto}`}
              </span>
            </div>
            <Anteprima
              css={(esito?.errori.length ?? 0) > 0 ? (ultimoValido?.css ?? "") : (esito?.css ?? "")}
              id={String(documento?.["id"] ?? id)}
              parti={parti}
              sondaAccesa={sonda}
              scelta={parteScelta}
              tema={tema === "light" ? "light" : undefined}
              densita={impaginazione?.density}
              movimento={impaginazione?.motion}
              onScegli={setParteScelta}
              onNodo={setNodoSotto}
              onLarghezza={setLarghezza}
            >
              <Impaginazione albero={albero} contesto={finto} slot={SLOT_FINTI} />
            </Anteprima>
            {documento !== null && documento["layout"] === undefined && (
              <p className="spiega-sonda section-card">
                <strong>Questa skin non dichiara un layout.</strong> Quel che
                vedi è l&apos;albero di serie: la prima modifica lo scrive per
                esteso nel documento — ed è corretto, nel momento in cui tocchi
                il layout il documento deve dire qual è.
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
      <footer className="piede-studio">
        <span className="voce-piede" data-esito={errori > 0 ? "male" : "bene"}>
          <Icona nome={errori > 0 ? "i-alert" : "i-check"} dim={14} />
          {errori} errori
        </span>
        <span className="voce-piede" data-esito={avvisi > 0 ? "attenzione" : undefined}>
          <Icona nome={avvisi > 0 ? "i-alert" : "i-check"} dim={14} />
          {avvisi} avvisi
        </span>
        {fuoriBudget > 0 && (
          <span className="voce-piede" data-esito="male">
            <Icona nome="i-alert" dim={14} />
            {fuoriBudget} superfici fuori budget
          </span>
        )}
        {sottoSoglia > 0 && (
          <span className="voce-piede" data-esito="male">
            <Icona nome="i-alert" dim={14} />
            {sottoSoglia} coppie sotto {registro?.contrastoMinimo ?? 4.5}:1
          </span>
        )}
        <span className="spinta" />
        <span className="mono">
          formato {registro?.format ?? 1} · {esito?.parti ?? 0} parti ridisegnate ·
          compilato in {esito?.compilatoMs ?? 0} ms
        </span>
      </footer>
    </section>
  );
}
