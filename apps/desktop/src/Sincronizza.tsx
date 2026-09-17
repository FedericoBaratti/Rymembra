/**
 * L'editor a battute: sincronizzare un testo premendo un tasto.
 *
 * # Perché esiste
 *
 * Perché nessun catalogo gratuito ha i testi di *tutte* le canzoni — quel
 * catalogo non esiste — e senza questa schermata la copertura si ferma dove si
 * ferma LRCLIB. Con questa, ogni brano è chiudibile: tre minuti di battute, e
 * quel che resta scoperto è solo quel che nessuno ha ancora sincronizzato.
 *
 * # Come si batte
 *
 * Si preme **Spazio** all'inizio di ogni riga mentre il brano suona. È il modo
 * più veloce che esista, e produce tempi sistematicamente in ritardo di due o
 * tre decimi di secondo: fra l'orecchio e il dito passa quello.
 *
 * La correzione non è un numero fisso — quel ritardo cambia da persona a
 * persona e con la stanchezza — ed è per questo che le battute non si salvano
 * come sono. Vanno al nucleo, che decodifica il brano, ne tira fuori gli
 * **attacchi** (dove il suono comincia davvero) e aggancia ogni battuta al più
 * vicino, correggendo le altre della latenza mediana misurata su quelle
 * agganciate. La regola sta in `aether_domain::testo::aggancia` e si prova
 * senza aprire una finestra; qui si raccolgono le pressioni di un tasto.
 *
 * # I momenti, e uno solo alla volta
 *
 * 1. **Le righe** — si incolla o si corregge il testo, una riga per verso.
 * 2. **Le battute** — si suona e si preme, una volta per riga.
 * 3. **La revisione** — si guardano i tempi raddrizzati, si correggono a mano
 *    quelli che serve, si salva.
 * 4. **Le parole** — *facoltativo*: si ribatte una volta per parola, e il testo
 *    si accende parola per parola invece che verso per verso.
 * 5. **Il dono** — è salvato, e c'è l'offerta di restituirlo a LRCLIB.
 *
 * Separati e non tutti in una schermata perché in ognuno la tastiera vuol dire
 * una cosa diversa: nel primo Spazio è uno spazio, nel secondo e nel quarto è
 * una battuta. Metterli insieme vorrebbe dire un editor in cui non si può
 * scrivere.
 *
 * # Perché il quarto è facoltativo, e perché sta dopo il terzo
 *
 * Facoltativo perché costa quanto il secondo moltiplicato per il numero di
 * parole di un verso, e quel che si guadagna — l'illuminazione dentro la riga —
 * è una rifinitura, non la funzione. Un testo sincronizzato al verso è già
 * finito, e chiedere di battere trecento volte prima di poter salvare
 * vorrebbe dire che nessuno arriva in fondo.
 *
 * Dopo il terzo perché i tempi delle parole si appoggiano a quelli delle righe:
 * una parola non può cominciare prima del verso a cui appartiene, e finché i
 * tempi dei versi si muovono quel confine si muove con loro.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useFinestrella } from "./finestrella";
import { ipc, testoErrore, type Brano, type TestoBrano } from "./ipc";
import { durata, nomeArtista } from "./formato";
import { t } from "./lingue";
import { Icona } from "./parti/Icone";
import { posizioneAdesso } from "./riproduzione";

/** Di quanto si torna indietro quando si riparte da una riga. */
const RINCORSA_MS = 2000;

/** Di quanto sposta una freccia, e di quanto con lo Shift premuto. */
const PASSO_FINE = 10;
const PASSO_GROSSO = 100;

/** In quale dei cinque momenti si è. */
type Momento = "righe" | "battute" | "rivedi" | "parole" | "dono";

/**
 * Le parole di una riga, con lo spazio che le segue attaccato a ciascuna.
 *
 * Gli spazi in coda non sono un dettaglio: concatenando le parole si deve
 * riottenere la riga, ed è **esattamente** la verifica che il nucleo fa prima
 * di scriverle (`aether_domain::testo::parole_combaciano`). Uno `split(" ")`
 * che li butta darebbe parole che rimesse in fila si attaccano fra loro, e il
 * nucleo le rifiuterebbe tutte — giustamente.
 *
 * Un `match` e non uno `split` per la stessa ragione: `split` su una riga con
 * due spazi di fila produce una parola vuota, che non è una parola e non ha
 * niente da battere.
 */
function spezza(riga: string): string[] {
  return riga.match(/\S+\s*/g) ?? [];
}

export function Sincronizza({
  brano,
  inCorso,
  iniziale,
  onChiudi,
  onSalvato,
  onErrore,
}: {
  brano: Brano;
  /**
   * Il brano che suona adesso, che non è per forza `brano`.
   *
   * Tre minuti di battute sono abbastanza perché il brano finisca e parta il
   * successivo: capita a chi batte l'ultima riga a ridosso della fine e poi
   * cerca Invio. Da lì la posizione del lettore è quella di un'altra canzone.
   * Una battuta presa lì sarebbe un tempo senza senso, e ogni salto — «rifai
   * l'ultima», «senti da qui» — salterebbe dentro il brano sbagliato. Finché
   * suona un altro brano, quindi, non si batte e non si salta: l'editor lo dice
   * e offre di tornare.
   */
  inCorso: number;
  /**
   * Il testo da cui partire: quel che il pannello aveva, o niente.
   *
   * Le **parole**, e soltanto quelle. Nessun offset arriva fin qui, e non
   * serve: i tempi si ribattono tutti, quindi quel che un eventuale `.lrc` di
   * partenza dichiarava in testa descriveva tempi che dopo questo passaggio non
   * esistono più. Vedi `testo_salva`, che spiega perché conservarlo sfaserebbe
   * ogni riga appena battuta.
   */
  iniziale: string;
  onChiudi: () => void;
  onSalvato: (testo: TestoBrano) => void;
  onErrore: (e: unknown) => void;
}) {
  const [momento, setMomento] = useState<Momento>("righe");
  const [grezzo, setGrezzo] = useState(iniziale);
  const [righe, setRighe] = useState<string[]>([]);
  const [tempi, setTempi] = useState<number[]>([]);
  const [indice, setIndice] = useState(0);
  // I tempi delle parole stanno in un vettore **piatto**, parallelo a `posti`,
  // e non annidato per riga: così battere una parola e battere una riga sono la
  // stessa operazione su due vettori diversi, e `batti`, `rifai` e `finisci`
  // servono tutt'e due i momenti invece di essere scritti due volte. Il
  // rimescolamento per riga si fa una volta sola, al salvataggio.
  const [tempiParola, setTempiParola] = useState<number[]>([]);
  const [indiceParola, setIndiceParola] = useState(0);
  const [raddrizzo, setRaddrizzo] = useState(false);
  const [salvando, setSalvando] = useState(false);
  // Il dono ha tre stati e non due: «non ancora», «sto mandando» e «mandato».
  // Il secondo dura secondi — la prova di lavoro di LRCLIB è calcolo, non
  // attesa — e senza un modo di dirlo sembrerebbe che il pulsante è rotto.
  const [donando, setDonando] = useState(false);
  const [donato, setDonato] = useState(false);
  // Se le richieste al catalogo sono spente, l'offerta non si fa: proporre di
  // mandare qualcosa a un servizio che si è deciso di non interpellare è una
  // domanda a cui la risposta è già stata data.
  const [reteAccesa, setReteAccesa] = useState(false);
  // Il file accanto al brano non si è scritto — una cartella di sola lettura,
  // una share — ma il testo è salvo nella libreria. Si dice nel «fatto», con la
  // ragione, invece di dire «non salvato» a chi il lavoro l'ha salvato.
  const [nonNelFile, setNonNelFile] = useState<string | null>(null);
  // La riga su cui si sta battendo, per tenerla in vista senza ridisegnare
  // l'elenco a ogni fotogramma.
  const corrente = useRef<HTMLLIElement | null>(null);
  const finestrella = useFinestrella<HTMLDivElement>(onChiudi);

  useEffect(() => {
    corrente.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [indice, indiceParola]);

  // È l'unica delle dieci finestrelle che si rifà il corpo da capo mentre è
  // aperta: il tasto che ha fatto cambiare momento si smonta subito dopo averlo
  // premuto, e Chromium **non** emette `blur` né `focusout` per un nodo
  // focalizzato che sparisce — nessun ascoltatore può accorgersene, e per questo
  // la cosa si rimedia qui e non dentro l'hook. Il fuoco casca sul `<body>`,
  // cioè fuori dalla finestrella, dove non passa né l'Escape né la trappola del
  // Tab, che `useFinestrella` ascolta sul nodo e non su `window`.
  // Si rimette sulla radice, che l'hook tiene focalizzabile di proposito:
  // non si vede (il contorno è `:focus-visible`, e un fuoco dato dal programma
  // dopo un clic non lo accende) e i due tasti tornano a funzionare. Quel che
  // resta su `window` — Spazio, Backspace, Invio del momento delle battute —
  // non cambia: per quei tre la radice e il `<body>` sono la stessa cosa.
  useEffect(() => {
    const dentro = finestrella.current;
    if (dentro !== null && !dentro.contains(document.activeElement))
      dentro.focus();
  }, [momento, finestrella]);

  // ── il momento delle righe ────────────────────────────────────────────────

  const cominciaABattere = useCallback(() => {
    // Le righe vuote in mezzo sono pause fra le strofe e si tengono: hanno un
    // loro tempo, e toglierle attaccherebbe l'ultima riga di una strofa alla
    // prima della successiva. Quelle in testa e in coda no.
    const pulite = grezzo.split("\n").map((riga) => riga.trim());
    while (pulite.length > 0 && pulite[0] === "") pulite.shift();
    while (pulite.length > 0 && pulite[pulite.length - 1] === "") pulite.pop();
    if (pulite.length === 0) return;
    setRighe(pulite);
    setTempi([]);
    setIndice(0);
    // I tempi delle parole se ne vanno con le righe, ed è la prima trappola di
    // questo pacchetto: sono tempi battuti su un testo, e questo testo può non
    // essere più quello. Riassegnarli per posizione darebbe un file che sembra
    // giusto e canta storto — si buttano, e chi le vuole le ribatte.
    setTempiParola([]);
    setIndiceParola(0);
    setMomento("battute");
  }, [grezzo]);

  // ── le parole da battere ──────────────────────────────────────────────────

  /* L'elenco piatto di quel che si batte nel quarto momento: una voce per
     parola, nell'ordine in cui si canta.

     Le righe di **una parola sola** non ci sono, ed è la seconda trappola di
     questo pacchetto: la loro unica parola comincia quando comincia la riga —
     il tempo si sa già — e farla battere vorrebbe dire chiedere una pressione
     che non aggiunge niente. Su un ritornello di monosillabi sarebbero decine.
     Il tempo gliel'assegna il salvataggio, gratis.

     Le righe vuote — le pause fra le strofe — non hanno parole e spariscono da
     sé. */
  const posti = useMemo(
    () =>
      righe.flatMap((riga, quale) => {
        const parole = spezza(riga);
        if (parole.length < 2) return [];
        return parole.map((testo, dove) => ({ riga: quale, dove, testo }));
      }),
    [righe],
  );

  // ── il momento delle battute, e quello delle parole ───────────────────────

  /* I tre gesti valgono in tutt'e due i momenti che si battono, e sono gli
     stessi tre: si preme, si disfa l'ultima, si raddrizza. Quel che cambia è
     **su quale vettore** si scrive — le righe o le parole — e le tre funzioni
     lo scelgono da `momento` invece di esistere in due copie. La copia sarebbe
     stata la strada breve, e sarebbe divergente al primo ritocco: il giorno in
     cui la rincorsa di `rifai` cambia, cambierebbe in un momento solo. */

  const siBattonoParole = momento === "parole";
  const altrove = inCorso !== brano.id;
  // Da dove si riparte tornando a battere le righe: con la rincorsa, poco prima
  // dell'ultima battuta; dall'inizio, se non ce n'è ancora nessuna.
  const ripresa = indice > 0 ? (tempi[indice - 1] ?? 0) - RINCORSA_MS : 0;

  /* Rimette il lettore su questo brano, e da `ms`, poi suona.

     Il brano si cerca nella coda come si vede — l'ordine in cui `coda_vai`
     conta — all'indietro dal brano in corso: il caso di tutti i giorni è che
     sia finito e sia partito il successivo, cioè che stia una riga sopra. Se
     nella coda non c'è più lo si mette subito dopo il brano in corso e si va
     lì: sostituire la coda per tornare a una canzone butterebbe via quel che
     qualcuno aveva messo in fila.

     Lo stato si chiede adesso e non si porta giù come prop: serve una volta,
     in risposta a un tasto, e un pannello che si ridisegna a ogni riordino
     della coda per un bottone che quasi nessuno preme sarebbe il prezzo
     sbagliato. */
  const suonaDa = useCallback(
    async (ms: number) => {
      if (altrove) {
        const stato = await ipc.riproduzioneStato();
        if (stato.brano?.id !== brano.id) {
          const qui = Math.min(
            stato.posizioneCoda ?? stato.coda.length,
            stato.coda.length - 1,
          );
          let dove = stato.coda.lastIndexOf(brano.id, qui);
          if (dove < 0) dove = stato.coda.indexOf(brano.id);
          if (dove < 0) {
            await ipc.codaDopo([brano.id]);
            const dopo = await ipc.riproduzioneStato();
            dove = dopo.coda.indexOf(brano.id, dopo.posizioneCoda ?? 0);
          }
          if (dove >= 0) await ipc.codaVai(dove);
        }
      }
      await ipc.vaiA(Math.max(0, ms));
      await ipc.riprendi();
    },
    [altrove, brano.id],
  );

  const batti = useCallback(() => {
    const quante = siBattonoParole ? posti.length : righe.length;
    const dove = siBattonoParole ? indiceParola : indice;
    if (dove >= quante || altrove) return;
    // `posizioneAdesso` e non la posizione iscritta: qui non si disegna a venti
    // fotogrammi al secondo, si legge una volta in risposta a un tasto. È
    // esattamente il caso per cui quella funzione esiste.
    //
    // Ed è la posizione **grezza**, come al momento delle righe: né
    // l'`[offset:]` del file né lo scarto di chi ascolta entrano qui dentro. Il
    // perché per esteso sta su `testo_salva`, che li azzera tutt'e due per la
    // stessa ragione — una battuta registrata su una posizione corretta nasce
    // spostata della latenza, e resta spostata per sempre.
    const adesso = posizioneAdesso();
    if (siBattonoParole) {
      setTempiParola((prima) => [...prima.slice(0, dove), adesso]);
      setIndiceParola((prima) => prima + 1);
      return;
    }
    setTempi((prima) => [...prima.slice(0, dove), adesso]);
    setIndice((prima) => prima + 1);
  }, [siBattonoParole, indice, indiceParola, posti.length, righe.length, altrove]);

  const rifai = useCallback(() => {
    const dove = siBattonoParole ? indiceParola : indice;
    if (dove === 0) return;
    const precedente = dove - 1;
    const scorsi = siBattonoParole ? tempiParola : tempi;
    if (siBattonoParole) {
      setIndiceParola(precedente);
      setTempiParola((prima) => prima.slice(0, precedente));
    } else {
      setIndice(precedente);
      setTempi((prima) => prima.slice(0, precedente));
    }
    // Con un altro brano in corso si disfa e basta: il salto cadrebbe dentro
    // quello. La rincorsa la dà il ritorno, dall'avviso.
    if (altrove) return;
    const da = Math.max(0, (scorsi[precedente] ?? 0) - RINCORSA_MS);
    ipc.vaiA(da).catch(onErrore);
  }, [siBattonoParole, indice, indiceParola, tempi, tempiParola, altrove, onErrore]);

  /* Il raddrizzamento agli attacchi non sa cosa sia una riga: prende un vettore
     di tempi e lo aggancia a dove il suono comincia davvero. Le parole sono
     tempi come gli altri — anzi sono il caso per cui gli attacchi esistono —
     quindi si manda lo stesso comando, e non ce n'è un secondo. */
  const finisci = useCallback(() => {
    const perParole = siBattonoParole;
    setRaddrizzo(true);
    ipc
      .pausa()
      .catch(() => {})
      .finally(() => {
        ipc
          .testoAggancia(brano.id, perParole ? tempiParola : tempi)
          .then((raddrizzati) => {
            if (!perParole) {
              setTempi(raddrizzati);
              setMomento("rivedi");
              return;
            }
            // Nessuna parola prima del suo verso: l'aggancio guarda gli
            // attacchi del brano e delle righe non sa niente, quindi la prima
            // parola di un verso può ritrovarsi un attimo prima di lui. Un
            // tempo così non è sbagliato di molto, ma è un tempo che non vuol
            // dire niente, e il file lo porterebbe in giro.
            setTempiParola(
              raddrizzati.map((ms, quale) =>
                Math.max(ms, tempi[posti[quale]?.riga ?? 0] ?? 0),
              ),
            );
          })
          .catch(onErrore)
          .finally(() => setRaddrizzo(false));
      });
  }, [siBattonoParole, brano.id, tempi, tempiParola, posti, onErrore]);

  // La tastiera vale solo mentre si batte — righe o parole: negli altri momenti
  // Spazio è uno spazio e le frecce muovono un cursore.
  const tutteBattute =
    momento === "parole"
      ? indiceParola >= posti.length && posti.length > 0
      : indice >= righe.length && righe.length > 0;

  useEffect(() => {
    if (momento !== "battute" && momento !== "parole") return;
    // In cattura, e fermato: le scorciatoie globali ascoltano anche loro su
    // `window`, e si erano iscritte prima. Senza, ogni battuta di Spazio segnava
    // la riga **e** metteva in pausa il brano che si stava battendo — cioè la
    // battuta dopo cadeva su una musica ferma.
    const alTasto = (evento: KeyboardEvent) => {
      const prendi = () => {
        evento.preventDefault();
        evento.stopPropagation();
      };
      if (evento.key === " ") {
        prendi();
        batti();
      } else if (evento.key === "Backspace") {
        prendi();
        rifai();
      } else if (evento.key === "Enter" && tutteBattute) {
        prendi();
        finisci();
      }
    };
    window.addEventListener("keydown", alTasto, { capture: true });
    return () =>
      window.removeEventListener("keydown", alTasto, { capture: true });
  }, [momento, batti, rifai, finisci, tutteBattute]);

  // ── il momento della revisione ────────────────────────────────────────────

  const sposta = useCallback((quale: number, quanto: number) => {
    setTempi((prima) =>
      prima.map((ms, i) => (i === quale ? Math.max(0, ms + quanto) : ms)),
    );
  }, []);

  // ── il momento delle parole ───────────────────────────────────────────────

  const cominciaLeParole = useCallback(() => {
    setTempiParola([]);
    setIndiceParola(0);
    setMomento("parole");
  }, []);

  /* Il ritocco delle parole è **solo** ±10 ms, mentre quello delle righe ha
     anche il passo da cento: dentro un verso cento millisecondi sono già una
     sillaba, e un pulsante che salta la parola accanto non serve a nessuno. */
  const spostaParola = useCallback(
    (quale: number, quanto: number) => {
      const minimo = tempi[posti[quale]?.riga ?? 0] ?? 0;
      setTempiParola((prima) =>
        prima.map((ms, i) =>
          i === quale ? Math.max(minimo, ms + quanto) : ms,
        ),
      );
    },
    [tempi, posti],
  );

  /* Si riparte dall'inizio del verso su cui si sta battendo, con la rincorsa.

     Senza rincorsa la prima parola sarebbe già passata nell'istante in cui il
     suono comincia — comincia esattamente lì — e la si perderebbe ogni volta.
     Due secondi sono la coda del verso precedente: si sente arrivare, e la mano
     è pronta. È lo stesso numero della disfatta, e per la stessa ragione. */
  const suonaDaQui = useCallback(() => {
    const dove = Math.min(indiceParola, posti.length - 1);
    const quale = posti[Math.max(0, dove)]?.riga ?? 0;
    suonaDa((tempi[quale] ?? 0) - RINCORSA_MS).catch(onErrore);
  }, [indiceParola, posti, tempi, suonaDa, onErrore]);

  /* Quali righe portano le parole, quando si salva.

     **Solo quelle battute per intero**, ed è la prima trappola di questo
     pacchetto. Una riga lasciata a metà avrebbe meno tempi che parole, e i
     tempi che mancano non si inventano: distribuirli sarebbe plausibile a
     leggerli e sbagliato ad ascoltarli. Il nucleo le butterebbe comunque —
     `parole_combaciano` verifica che le parole ricompongano la riga — e mandare
     qualcosa che si sa verrà buttato è solo un modo di non sapere cosa succede.

     Le righe di **una parola sola** non si battono, ma la parola il suo tempo
     ce l'ha: è quello della riga. Si scrive solo quando qualche altra riga le
     parole le ha davvero, perché un `.a2.lrc` in cui l'unica cosa timbrata sono
     i monosillabi non è un `a2` — è un `.lrc` con del rumore dentro. */
  const parolePerRiga = useCallback((): {
    ms: number;
    testo: string;
  }[][] => {
    const per: { ms: number; testo: string }[][] = righe.map(() => []);
    const ultimo = new Map<number, number>();
    posti.forEach((posto, quale) => ultimo.set(posto.riga, quale));
    const finite = new Set(
      [...ultimo].filter(([, quale]) => quale < indiceParola).map(([riga]) => riga),
    );
    if (finite.size === 0) return per;
    posti.forEach((posto, quale) => {
      if (!finite.has(posto.riga)) return;
      // Il confine si riapplica qui e non solo dopo l'aggancio: fra il quarto
      // momento e il salvataggio si può tornare alla revisione e spostare un
      // verso di cento millisecondi, e allora le sue parole starebbero prima
      // di lui.
      const inizio = tempi[posto.riga] ?? 0;
      per[posto.riga]?.push({
        ms: Math.max(inizio, tempiParola[quale] ?? inizio),
        testo: posto.testo,
      });
    });
    righe.forEach((riga, quale) => {
      const sola = spezza(riga);
      if (sola.length === 1)
        per[quale] = [{ ms: tempi[quale] ?? 0, testo: sola[0] ?? riga }];
    });
    return per;
  }, [righe, posti, tempi, tempiParola, indiceParola]);

  const righeConParole = useMemo(
    () => parolePerRiga().filter((quali) => quali.length > 0).length,
    [parolePerRiga],
  );

  const salva = useCallback(() => {
    setSalvando(true);
    const parole = parolePerRiga();
    ipc
      .testoSalva(
        brano.id,
        righe.map((testo, i) => ({
          ms: tempi[i] ?? 0,
          testo,
          parole: parole[i] ?? [],
        })),
      )
      .then((salvato) => {
        // Il pannello dietro si aggiorna subito, prima ancora che questa
        // finestra si chiuda. Il lavoro è finito e salvato; quel che resta è
        // un'offerta, e un'offerta non deve tenere in ostaggio il risultato.
        onSalvato(salvato.testo);
        setNonNelFile(
          salvato.fileNonScritto === null
            ? null
            : testoErrore(salvato.fileNonScritto),
        );
        return ipc
          .testiStato()
          .then((stato) => setReteAccesa(stato.rete))
          .catch(() => setReteAccesa(false))
          .then(() => setMomento("dono"));
      })
      .catch(onErrore)
      .finally(() => setSalvando(false));
  }, [brano.id, righe, tempi, parolePerRiga, onSalvato, onErrore]);

  /* Restituire. È un gesto separato dal salvataggio per una ragione sola: si
     salva sempre, quindi tutto quel che sta attaccato al salvataggio è
     automatico, e mandare il proprio lavoro a un servizio pubblico non deve
     succedere per inerzia. Chi non preme qui non ha mandato niente, e il `.lrc`
     è comunque sul disco. */
  const dona = useCallback(() => {
    setDonando(true);
    ipc
      .testoPubblica(brano.id)
      .then(() => setDonato(true))
      .catch(onErrore)
      .finally(() => setDonando(false));
  }, [brano.id, onErrore]);

  return (
    <div className="velo scuro">
      <div
        ref={finestrella}
        className="finestrella larga sincronizza glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("sync.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{t("sync.aria")}</h2>
        <div className="percorso">
          {brano.title} · {nomeArtista(brano.artist)}
        </div>

        {/* Suona un altro brano: le battute sono ferme, e si dice qui e non
            con un tasto che smette di rispondere. Il ritorno riparte da dove
            serve in quel momento — la rincorsa dell'ultima battuta, l'inizio
            del verso delle parole — e nella revisione, dove si ascolta riga per
            riga, dall'inizio. */}
        {altrove && momento !== "righe" && momento !== "dono" && (
          <div className="altrove" role="status">
            <span>{t("sync.elsewhere")}</span>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={() =>
                momento === "parole"
                  ? suonaDaQui()
                  : suonaDa(momento === "battute" ? ripresa : 0).catch(onErrore)
              }
            >
              <Icona nome="i-play" dim={12} />
              {t("sync.elsewhere.back")}
            </button>
          </div>
        )}

        {momento === "righe" && (
          <>
            <p className="nota">{t("sync.lines.help")}</p>
            <textarea
              className="righe-da-battere"
              value={grezzo}
              rows={14}
              spellCheck={false}
              onChange={(e) => setGrezzo(e.target.value)}
              aria-label={t("sync.lines.aria")}
            />
            <div className="azioni">
              <button type="button" className="bottone btn-ghost" onClick={onChiudi}>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={grezzo.trim() === ""}
                onClick={cominciaABattere}
              >
                {t("sync.lines.start")}
              </button>
            </div>
          </>
        )}

        {momento === "battute" && (
          <>
            <p className="nota">
              {tutteBattute ? t("sync.tap.done") : t("sync.tap.help")}
            </p>
            <ol className="elenco-battute">
              {righe.map((riga, i) => (
                <li
                  key={`${i}-${riga}`}
                  ref={i === indice ? corrente : undefined}
                  /* La riga che aspetta la battuta si dice, non solo si
                     dipinge: `data-attesa` la colora e a chi ascolta lo
                     schermo non arriva. `aria-current` perché è la stessa
                     informazione che `.riga` usa per il brano in corso. */
                  aria-current={i === indice ? "true" : undefined}
                  data-attesa={i === indice || undefined}
                  data-fatta={i < indice || undefined}
                >
                  <span className="quando">
                    {i < indice ? durata(tempi[i] ?? 0) : "—"}
                  </span>
                  <span className="cosa">{riga === "" ? " " : riga}</span>
                </li>
              ))}
            </ol>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={() =>
                  (altrove ? suonaDa(ripresa) : ipc.alterna()).catch(onErrore)
                }
              >
                <Icona nome="i-play" dim={15} />
                {t("sync.tap.play")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                disabled={indice === 0}
                onClick={rifai}
              >
                {t("sync.tap.undo")}
                <kbd className="scorciatoia">⌫</kbd>
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={!tutteBattute || raddrizzo}
                onClick={finisci}
              >
                {raddrizzo ? t("sync.tap.straightening") : t("sync.tap.finish")}
              </button>
            </div>
            <p className="nota">{t("sync.tap.key")}</p>
          </>
        )}

        {momento === "rivedi" && (
          <>
            <p className="nota">{t("sync.review.help")}</p>
            <ol className="elenco-battute rivedi">
              {righe.map((riga, i) => (
                <li key={`${i}-${riga}`}>
                  <span className="quando">{durata(tempi[i] ?? 0)}</span>
                  <span className="cosa">{riga === "" ? " " : riga}</span>
                  <span className="ritocco">
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.earlier")}
                      onClick={() => sposta(i, -PASSO_GROSSO)}
                    >
                      −{PASSO_GROSSO}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.earlier.fine")}
                      onClick={() => sposta(i, -PASSO_FINE)}
                    >
                      −{PASSO_FINE}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.later.fine")}
                      onClick={() => sposta(i, PASSO_FINE)}
                    >
                      +{PASSO_FINE}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.later")}
                      onClick={() => sposta(i, PASSO_GROSSO)}
                    >
                      +{PASSO_GROSSO}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.hear")}
                      /* Il titolo resta per il mouse; il nome porta il numero
                         della riga, perché un elenco di trenta bottoni che si
                         chiamano tutti «Senti da qui» non dice da dove. */
                      aria-label={t("sync.review.hearLine", {
                        riga: String(i + 1),
                      })}
                      onClick={() =>
                        suonaDa((tempi[i] ?? 0) - 1000).catch(onErrore)
                      }
                    >
                      <Icona nome="i-play" dim={12} />
                    </button>
                  </span>
                </li>
              ))}
            </ol>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={() => {
                  setIndice(0);
                  setTempi([]);
                  // Anche qui: i tempi delle parole si appoggiano a quelli dei
                  // versi, e i versi stanno per cambiare tutti.
                  setTempiParola([]);
                  setIndiceParola(0);
                  setMomento("battute");
                }}
              >
                {t("sync.review.again")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={onChiudi}
              >
                {t("common.cancel")}
              </button>
              {/* Il quarto momento è una porta, non un passaggio obbligato:
                  sta **prima** del primario, che resta «Salva». Chi non lo
                  apre ha finito, e il testo che salva è completo. Se nessuna
                  riga ha due parole il tasto non c'è affatto — un tasto
                  perennemente spento è una promessa che non si mantiene. */}
              {posti.length > 0 && (
                <button
                  type="button"
                  className="bottone btn-ghost"
                  onClick={cominciaLeParole}
                >
                  {t("sync.words.start")}
                </button>
              )}
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={salvando}
                onClick={salva}
              >
                {salvando ? t("sync.review.saving") : t("sync.review.save")}
              </button>
            </div>
            {righeConParole > 0 && (
              <p className="nota">
                {t("sync.words.attached", { n: righeConParole })}
              </p>
            )}
            {/* Dove finisce, detto prima di premere: un file accanto alla
                musica lo leggono anche gli altri lettori, e chi non lo vuole
                deve poterlo sapere adesso e non dopo. */}
            <p className="nota">{t("sync.review.where")}</p>
          </>
        )}

        {momento === "parole" && (
          <>
            <p className="nota">
              {tutteBattute ? t("sync.words.done") : t("sync.words.help")}
            </p>
            {/* Lo stesso elenco delle battute, con le parole al posto dei
                versi: `data-attesa`, `data-fatta` e le due colonne vengono da
                lì. `data-capo` marca la prima parola di ogni verso, ed è
                l'unica cosa che dice dove finisce una riga e comincia
                l'altra — le parole sono in fila, e senza quel segno un
                ritornello sarebbe una colonna indistinta. */}
            <ol className="elenco-battute parole">
              {posti.map((posto, quale) => (
                <li
                  key={`${posto.riga}-${posto.dove}`}
                  ref={quale === indiceParola ? corrente : undefined}
                  aria-current={quale === indiceParola ? "true" : undefined}
                  data-attesa={quale === indiceParola || undefined}
                  data-fatta={quale < indiceParola || undefined}
                  data-capo={posto.dove === 0 || undefined}
                >
                  <span className="quando">
                    {quale < indiceParola ? durata(tempiParola[quale] ?? 0) : "—"}
                  </span>
                  <span className="cosa">{posto.testo}</span>
                  <span className="ritocco">
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.earlier.fine")}
                      disabled={quale >= indiceParola}
                      onClick={() => spostaParola(quale, -PASSO_FINE)}
                    >
                      −{PASSO_FINE}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.later.fine")}
                      disabled={quale >= indiceParola}
                      onClick={() => spostaParola(quale, PASSO_FINE)}
                    >
                      +{PASSO_FINE}
                    </button>
                  </span>
                </li>
              ))}
            </ol>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={() => setMomento("rivedi")}
              >
                {t("sync.words.back")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={suonaDaQui}
              >
                <Icona nome="i-play" dim={15} />
                {t("sync.words.play")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                disabled={indiceParola === 0}
                onClick={rifai}
              >
                {t("sync.tap.undo")}
                <kbd className="scorciatoia">⌫</kbd>
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                disabled={!tutteBattute || raddrizzo}
                onClick={finisci}
              >
                {raddrizzo ? t("sync.tap.straightening") : t("sync.tap.finish")}
              </button>
              {/* Salvare si può in qualunque momento, e non è una svista: quel
                  che è battuto per intero si scrive, il resto resta
                  sincronizzato al verso. Un passo facoltativo che tiene in
                  ostaggio il salvataggio finché non è finito non è
                  facoltativo. */}
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={salvando}
                onClick={salva}
              >
                {salvando ? t("sync.review.saving") : t("sync.review.save")}
              </button>
            </div>
            <p className="nota">{t("sync.words.key")}</p>
            <p className="nota">{t("sync.words.where")}</p>
          </>
        )}

        {momento === "dono" && (
          <div className="dono">
            <p className="fatto">
              <Icona nome="i-check" dim={16} />
              {nonNelFile === null
                ? t("sync.done.saved")
                : t("sync.done.savedInLibrary")}
            </p>
            {nonNelFile !== null && <p className="nota">{nonNelFile}</p>}

            {/* Senza rete verso il catalogo non c'è offerta: si dice che è
                fatto e si chiude, senza far intravedere una porta chiusa. */}
            {reteAccesa && !donato && (
              <>
                <p className="nota">{t("sync.done.offer")}</p>
                {/* Cosa esce di qui, per esteso e prima di premere: il titolo
                    che il catalogo userà per ritrovarlo, e le righe con i loro
                    tempi. Non c'è niente d'altro nell'invio — nessun percorso,
                    nessun identificativo, niente sul dispositivo.

                    Artista e album restano **grezzi**, e qui è giusto così:
                    `nomeArtista` e `titoloAlbum` traducono le sentinelle del
                    nucleo per chi guarda, ma quel che parte per LRCLIB è il
                    valore in tabella — `testi::da_restituire` legge `tracks` e
                    manda `brano.artist` come sta. Tradurli in questo elenco
                    vorrebbe dire scrivere «Unknown artist» accanto a una
                    richiesta che porta «Artista sconosciuto»: l'unico posto
                    dell'interfaccia che promette di dire i byte esatti
                    diventerebbe l'unico che non li dice. */}
                <ul className="cosa-va">
                  <li>
                    {brano.title} · {brano.artist}
                    {brano.album ? ` · ${brano.album}` : ""}
                  </li>
                  <li>{t("sync.done.lines", { n: righe.length })}</li>
                </ul>
                <p className="nota">{t("sync.done.slow")}</p>
              </>
            )}

            {donato && <p className="nota">{t("sync.done.thanks")}</p>}

            <div className="azioni">
              {reteAccesa && !donato && (
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={donando}
                  onClick={dona}
                >
                  {donando ? t("sync.done.giving") : t("sync.done.give")}
                </button>
              )}
              {/* Chiudere è il pulsante primario, anche qui. Chi arriva a
                  questa schermata ha già ottenuto quel che voleva; il dono è la
                  strada in più, non quella diritta. */}
              <button
                type="button"
                className="bottone primario btn-accent"
                onClick={onChiudi}
              >
                {t("common.close")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
