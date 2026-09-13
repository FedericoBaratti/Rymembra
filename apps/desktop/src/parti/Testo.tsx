/**
 * Il testo del brano in riproduzione, che scorre.
 *
 * # Quel che questo componente non fa
 *
 * Non interpreta niente. Le righe arrivano dal nucleo già in ordine e già in
 * millisecondi: qui non c'è nessuna espressione regolare, nessun `[mm:ss.xx]` e
 * nessuna idea di cosa sia un LRC. È deliberato — il lettore vero sta in
 * `aether_domain::testo`, si prova senza aprire una finestra, e un secondo
 * lettore scritto qui divergerebbe dal primo su tutto quel che il formato non
 * dice.
 *
 * # Perché è una foglia, e cosa costa
 *
 * Si iscrive a `usePosizioneMs`, cioè si ridisegna venti volte al secondo: è la
 * stessa scelta dello `Scrubber`, ed è per questo che l'archivio della posizione
 * vive fuori da React. Il costo si tiene in due modi:
 *
 * * l'elenco delle righe è memoizzato su `[righe, attiva, …]`, quindi venti
 *   volte al secondo React confronta **un riferimento**, non duecento
 *   paragrafi. Le righe si ricostruiscono solo quando la riga accesa cambia,
 *   cioè qualche decina di volte in una canzone;
 * * l'avanzamento dentro la riga — l'illuminazione che l'attraversa — non passa
 *   da React: è una proprietà CSS scritta su un `ref`. Farne uno stato vorrebbe
 *   dire ricostruire l'albero per muovere un gradiente di un pixel.
 *
 * # L'illuminazione progressiva, e perché conta
 *
 * L'illuminazione vive su un elemento inline dentro la riga — `lyric-fill` —
 * e non sulla riga stessa, perché una riga che va a capo è un blocco solo con
 * due righe visive: un gradiente orizzontale ci cadrebbe sopra due volte in
 * parallelo invece di attraversarle in fila. Il perché esteso sta accanto
 * all'elemento, in fondo a questo file.
 *
 * Un LRC è sincronizzato **alla riga**: dice quando una riga comincia, non
 * quando comincia ogni parola. Illuminare la riga tutta insieme e lasciarla
 * ferma fino alla successiva è corretto e sembra rotto — l'occhio non ha niente
 * da seguire. Far attraversare l'illuminazione nel tempo che separa una riga
 * dalla successiva costa una variabile CSS e restituisce la sensazione del
 * karaoke, senza inventare nessun tempo che non ci sia.
 *
 * Quando i tempi delle parole invece ci sono — LRC esteso, `.a2.lrc` — il
 * fronte smette di attraversare la riga a velocità costante: si ferma su una
 * parola e riparte con la voce, perché è la voce che i tempi descrivono. È
 * l'unico caso in cui l'illuminazione dice qualcosa di vero invece di essere
 * una bugia gentile, e vale la seconda strada.
 *
 * Anche quella seconda strada non passa da React: sono le stesse scritture su
 * `ref`, una per parola invece di una per pannello. Dieci parole scritte venti
 * volte al secondo sono duecento scritture, che è quel che costa una riga di
 * `console.log` — e in cambio non si ricostruisce nessun albero.
 *
 * # Il tempo in cui non si canta
 *
 * Un LRC ha tempi anche dove non ci sono parole: l'introduzione prima della
 * prima riga, e le righe vuote fra due strofe. In quei momenti la regola di
 * sopra non ha niente da illuminare, e il pannello resta immobile con l'ultima
 * riga già cantata spenta e la prossima ancora spenta — cioè sembra rotto
 * esattamente quando invece sta funzionando.
 *
 * Al loro posto vanno tre puntini che si riempiono, guidati dallo **stesso**
 * `--avanzamento` di tutto il resto: l'introduzione è trattata come una riga
 * virtuale che comincia a zero, e una riga vuota è già una riga con il suo
 * tempo. Non c'è nessun meccanismo nuovo, c'è la stessa variabile letta da tre
 * elementi invece che da uno.
 *
 * Sotto i tre secondi non compaiono: uno stacco breve è un respiro, e tre
 * puntini che lampeggiano per un secondo e mezzo sono rumore. Resta la riga
 * vuota alta quanto una riga, che è quel che l'LRC dice.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ipc, type Brano, type RigaTesto, type TestoBrano } from "../ipc";
import { Sincronizza } from "../Sincronizza";
import { anticipoAdesso, usePosizioneMs } from "../riproduzione";
import { fermoRestando } from "../transizione";
import { t, tSe } from "../lingue";
import { Icona } from "./Icone";

/**
 * Di quanto sposta una spinta del cursore di correzione.
 *
 * Cento millisecondi. Sotto, la correzione non si sente e servirebbero venti
 * clic per raddrizzare un testo storto di mezzo secondo; sopra, si scavalca il
 * punto giusto e si torna indietro.
 */
const PASSO_SCARTO = 100;

/**
 * Per quanto lo scorrimento automatico sta fermo dopo che l'hai toccato tu.
 *
 * Sei secondi. Erano tre, e tre era il compromesso di quando la pausa non si
 * vedeva: abbastanza per finire una strofa, abbastanza poco da non lasciare
 * perso chi si era distratto. Adesso che «torna al brano» compare e dice sia
 * che il pannello è fermo sia come rimetterlo in moto, il ritorno automatico
 * non è più l'unica via d'uscita, e può permettersi di aspettare il doppio —
 * cioè di non interrompere chi sta leggendo la strofa dopo.
 */
const PAUSA_SCORRIMENTO = 6000;

/**
 * Quanto deve durare uno stacco perché valga la pena disegnarlo.
 *
 * Tre secondi. Sotto, il tempo che ci vuole a **notare** i puntini è già più di
 * quello che restano, e l'effetto è un lampeggìo fra una strofa e l'altra.
 */
const STACCO_MINIMO = 3000;

/**
 * Dove sta la riga accesa nel pannello: da 0 in cima a 1 in fondo.
 *
 * Poco sopra la metà, e non a metà esatta. Leggendo si guarda avanti — la riga
 * dopo importa più di quella prima — e un'ancora al 42% dà a quel che deve
 * ancora arrivare metà pannello invece di un quarto.
 */
const ANCORA = 0.42;

/**
 * Oltre quante righe di salto lo scorrimento smette di essere morbido.
 *
 * Due. Passare alla riga accanto scivolando è quel che rende leggibile il
 * movimento; attraversare mezza canzone scivolando è mezzo secondo in cui non
 * si legge niente e non si capisce dove si è finiti. Il caso non è raro: capita
 * ogni volta che il pannello si apre a metà brano, e ogni volta che si salta
 * col cursore.
 */
const SALTO_SECCO = 2;

/**
 * I tasti che scorrono, e nessun altro.
 *
 * Serve perché `onKeyDown` sul contenitore sente **tutto** quel che sale dai
 * figli: il tabulatore che se ne va, e l'Invio che salta a una riga. Se anche
 * quelli mettessero in pausa l'inseguimento, «torna al brano» comparirebbe per
 * un fotogramma a ogni salto — cioè il pannello annuncerebbe di essersi fermato
 * nell'atto di rimettersi in moto.
 *
 * La barra spaziatrice non c'e': su un bottone col fuoco non scorre niente, lo
 * preme. Questi sette invece scorrono e basta, comunque sia il fuoco.
 */
const TASTI_CHE_SCORRONO = new Set([
  "ArrowUp",
  "ArrowDown",
  "PageUp",
  "PageDown",
  "Home",
  "End",
]);

/**
 * Il ripiego di un elenco vuoto, uno solo.
 *
 * `testo?.righe ?? []` scriverebbe un array nuovo a ogni disegno, cioè venti al
 * secondo, e ognuno farebbe girare da capo gli effetti che hanno `righe` fra le
 * dipendenze. Un riferimento fisso costa una riga e li spegne tutti.
 */
const NESSUNA_RIGA: RigaTesto[] = [];

/**
 * Quale riga è accesa a questa posizione: la stessa ricerca binaria del nucleo.
 *
 * `anticipo` arriva da fuori, e fino a ieri era il numero `150` scritto qui. Due
 * copie a mano della stessa costante — questa e `aether_domain::testo::ANTICIPO_MS`
 * — sono durate finché nessuno ha toccato nessuna delle due; adesso il nucleo lo
 * manda dentro lo stato della riproduzione e `anticipoAdesso` lo legge di lì.
 *
 * Serve perché la riga si accenda un attimo prima del suo tempo: l'occhio deve
 * arrivarci prima della voce. L'avanzamento dentro la riga parte invece dal tempo
 * vero, quindi in quell'attimo la riga è accesa e ferma a zero.
 */
function rigaAttiva(
  righe: TestoBrano["righe"],
  posizione: number,
  anticipo: number,
): number {
  const soglia = posizione + anticipo;
  let basso = 0;
  let alto = righe.length;
  while (basso < alto) {
    const mezzo = (basso + alto) >> 1;
    if ((righe[mezzo]?.ms ?? 0) <= soglia) basso = mezzo + 1;
    else alto = mezzo;
  }
  return basso - 1;
}

/**
 * Quanto è avanzata la riga accesa, da 0 a 1.
 *
 * Prende due indici che non sono righe, e li prende di proposito:
 *
 * * `-1` è l'introduzione, cioè la riga virtuale che comincia a zero e finisce
 *   dove comincia la prima vera. `righe[-1 + 1]` è già `righe[0]`, quindi
 *   l'unica cosa da dire è che il suo inizio è zero;
 * * l'**ultima** riga non ha una successiva da cui misurare, e prende `fine` —
 *   la durata del brano. `aether_domain::testo::avanzamento` in quel caso torna
 *   zero, e ha ragione: il dominio non sa quanto è lungo il file. La finestra
 *   sì, e usarla non è inventare un tempo, è leggerne uno che c'è già. Senza,
 *   l'ultima riga di ogni canzone resta bianca e ferma per tutta la coda.
 */
function avanzamento(
  righe: TestoBrano["righe"],
  indice: number,
  posizione: number,
  fine: number,
): number {
  const inizio = indice < 0 ? 0 : righe[indice]?.ms;
  if (inizio === undefined) return 0;
  const finisce = righe[indice + 1]?.ms ?? fine;
  const campo = finisce - inizio;
  if (campo <= 0) return 0;
  const fatto = posizione - inizio;
  if (fatto <= 0) return 0;
  if (fatto >= campo) return 1;
  return fatto / campo;
}

/**
 * Quale parola è in bocca a questa posizione, o `-1` prima della prima.
 *
 * Senza anticipo, al contrario di [`rigaAttiva`]: la riga si accende prima
 * perché l'occhio ci deve arrivare, ma dentro la riga l'occhio è già arrivato e
 * una parola che si accende prima di essere cantata è solo sbagliata.
 */
function parolaAttiva(parole: RigaTesto["parole"], posizione: number): number {
  let basso = 0;
  let alto = parole.length;
  while (basso < alto) {
    const mezzo = (basso + alto) >> 1;
    if ((parole[mezzo]?.ms ?? 0) <= posizione) basso = mezzo + 1;
    else alto = mezzo;
  }
  return basso - 1;
}

/**
 * Quanto è avanzata una parola, da 0 a 1.
 *
 * L'ultima parola della riga finisce dove comincia la riga dopo. Quando la riga
 * dopo non c'è — è l'ultima del brano — la parola resta accesa a metà per
 * sempre, e allora si preferisce accesa del tutto: `fine` arriva già com'è, e
 * un campo non positivo vale uno invece di zero.
 */
function avanzamentoParola(
  parole: RigaTesto["parole"],
  indice: number,
  posizione: number,
  fine: number,
): number {
  const parola = parole[indice];
  if (!parola) return 0;
  const finisce = parole[indice + 1]?.ms ?? fine;
  const campo = finisce - parola.ms;
  if (campo <= 0) return 1;
  const fatto = posizione - parola.ms;
  if (fatto <= 0) return 0;
  if (fatto >= campo) return 1;
  return fatto / campo;
}

/**
 * I tre puntini di uno stacco.
 *
 * Non portano stato e non portano tempi: si riempiono leggendo `--avanzamento`,
 * che è già scritto sul pannello per la riga accesa. Il taglio in tre lo fa il
 * foglio di stile con tre `calc()` sulla stessa variabile — vedi `.lyric-breath`
 * — perché dividere un numero in tre parti è quel che il CSS sa fare da sé.
 */
function Stacco() {
  return (
    <span className="lyric-breath" role="img" aria-label={t("np.lyrics.gap")}>
      <span />
      <span />
      <span />
    </span>
  );
}

export function Testo({
  brano,
  onErrore,
}: {
  brano: Brano;
  onErrore: (e: unknown) => void;
}) {
  const [testo, setTesto] = useState<TestoBrano | null>(null);
  const [cercando, setCercando] = useState(false);
  // La richiesta al catalogo è caduta. Uno stato a parte da «non c'è», perché
  // sono due cose diverse e chiedono due frasi diverse: «il catalogo non lo
  // conosce» è una risposta e si accetta, «non si è potuto chiedere» è un
  // guasto e si riprova. Confonderle era il difetto — il pannello diceva
  // «Nessun testo per questo brano» anche quando il wifi era staccato.
  const [guasto, setGuasto] = useState(false);
  const [editor, setEditor] = useState(false);
  // Che lo scorrimento sia fermo lo sa già `fermoFino`, che è un `ref` perché
  // lo legge un effetto che gira venti volte al secondo. Questo stato esiste
  // solo perché la pilloletta si veda comparire e sparire: sono due disegni per
  // gesto, non venti al secondo.
  const [inPausa, setInPausa] = useState(false);
  const posizioneMs = usePosizioneMs();
  const contenitore = useRef<HTMLDivElement | null>(null);
  const rigaAccesa = useRef<HTMLElement | null>(null);
  const fermoFino = useRef(0);
  const scadenzaPausa = useRef(0);
  // Da quale riga si arriva, per decidere se il salto è di una riga o di mezza
  // canzone. Sta in un `ref` e non in uno stato perché serve **dentro**
  // l'effetto che scorre: farne uno stato vorrebbe dire un secondo disegno per
  // ogni riga, per un numero che nessuno disegna.
  const attivaPrima = useRef(-1);

  /* Chi tiene il riferimento alla riga accesa.
     Una funzione e non l'oggetto `ref` nudo perché la riga accesa è un
     `<button>` quando si può saltarci e un `<p>` quando è una pausa: un
     `RefObject<HTMLElement>` non si passa a nessuno dei due — i riferimenti
     sono invarianti nel loro contenuto — mentre una funzione che accetta
     l'antenato comune si passa a tutti e due. */
  const segna = useCallback((nodo: HTMLElement | null) => {
    rigaAccesa.current = nodo;
  }, []);

  /* Quale brano il pannello sta guardando adesso.
     Serve alle richieste al catalogo, che possono partire anche da un bottone
     e non solo dall'effetto qui sotto: là il guardiano è la chiusura di
     `annullato`, qui non c'è nessuna chiusura da chiudere, e l'unica domanda
     da fare quando la risposta arriva è «è ancora questo il brano?».

     Sta in un effetto suo, dichiarato **prima** di quello che chiede: gli
     effetti girano nell'ordine in cui stanno scritti, quindi al cambio di
     brano questo riferimento è già aggiornato quando la richiesta parte, e una
     risposta in ritardo del brano di prima trova un numero diverso. */
  const branoOra = useRef(brano.id);
  useEffect(() => {
    branoOra.current = brano.id;
  }, [brano.id]);

  /* La domanda al catalogo, da qualunque parte arrivi.
     Due vie, e la differenza non è di gusto: `testoCerca` è la domanda
     normale, che passa dalle due memorie — il deposito e `checked_at` — e
     `testoCercaDiNuovo` è il gesto di chi ha davanti un pannello vuoto e le
     salta tutt'e due. Il perché per esteso sta su `testo_cerca_di_nuovo`.

     Il guasto **non** sale a `onErrore`: una fascia rossa in cima
     all'applicazione per un testo che non è arrivato è sproporzionata, e
     soprattutto non offre l'unica cosa che serve, cioè riprovare. Il pannello
     la offre, accanto al testo che manca. */
  const chiedi = useCallback((id: number, diNuovo: boolean) => {
    setCercando(true);
    setGuasto(false);
    const domanda = diNuovo ? ipc.testoCercaDiNuovo(id) : ipc.testoCerca(id);
    return domanda
      .then((dalla_rete) => {
        if (branoOra.current === id) setTesto(dalla_rete);
      })
      .catch(() => {
        if (branoOra.current === id) setGuasto(true);
      })
      .finally(() => {
        if (branoOra.current === id) setCercando(false);
      });
  }, []);

  // Il testo si chiede a ogni cambio di brano. `annullato` è il guardiano
  // solito: chi cambia brano tre volte in due secondi ha tre richieste in volo,
  // e senza questo l'ultima a rispondere vincerebbe invece dell'ultima chiesta.
  useEffect(() => {
    let annullato = false;
    setTesto(null);
    setCercando(false);
    setGuasto(false);
    ipc
      .testoBrano(brano.id)
      .then((trovato) => {
        if (annullato) return;
        setTesto(trovato);
        // Quel che si ha non scorre, e non si è ancora chiesto a nessuno: è
        // l'unico caso in cui si va in rete, ed è la ragione per cui il
        // prelievo sta qui dentro invece che nel nucleo. Una passata di
        // sottofondo direbbe al catalogo cosa c'è nella libreria; questo gli
        // dice cosa si sta ascoltando adesso, e solo perché qualcuno ha aperto
        // il pannello. La condizione la decide il nucleo — vedi `daChiedere`:
        // un testo piatto conta come «non si ha», perché il catalogo tiene più
        // voci per brano e la prima che risponde non è sempre quella con i
        // tempi.
        if (!trovato.daChiedere) return;
        return chiedi(brano.id, false);
      })
      // Qui ci arriva solo `testoBrano`, che legge il disco e il database:
      // `chiedi` i propri guasti se li tiene, perché sono l'unico caso in cui
      // c'è qualcosa da riprovare. Un database che non risponde no.
      .catch((e: unknown) => {
        if (!annullato) onErrore(e);
      });
    return () => {
      annullato = true;
    };
  }, [brano.id, onErrore, chiedi]);

  // Un brano nuovo è un elenco nuovo: la riga da cui si arriva non esiste più,
  // e senza questo il primo inseguimento del brano nuovo si crederebbe vicino
  // a una riga dell'elenco di prima. Anche la pausa cade: l'hai chiesta per il
  // testo che stavi leggendo, non per questo.
  useEffect(() => {
    attivaPrima.current = -1;
    fermoFino.current = 0;
    window.clearTimeout(scadenzaPausa.current);
    setInPausa(false);
  }, [brano.id]);

  // Il timer della pilloletta non deve sopravvivere al pannello.
  useEffect(() => () => window.clearTimeout(scadenzaPausa.current), []);

  // La posizione con cui confrontare i tempi: quella del lettore più i due
  // scarti. È `aether_domain::testo::posizione_corretta`, e la regola è che i
  // tempi delle righe non si toccano mai — si sposta il metro, non i numeri.
  const corretta =
    posizioneMs + (testo?.offsetMs ?? 0) + (testo?.scartoMs ?? 0);
  const righe = testo?.righe ?? NESSUNA_RIGA;
  // L'anticipo si legge a ogni disegno e non si memoizza: è una lettura di una
  // variabile di modulo, e il numero può cambiare sotto — cambia quando il nucleo
  // manda uno stato, cioè qualche volta per canzone.
  const attiva =
    righe.length > 0 ? rigaAttiva(righe, corretta, anticipoAdesso()) : -1;
  // Dove finisce l'ultima riga, e l'ultima parola dentro di lei. Un brano di
  // durata ignota — non capita, ma il tipo lo ammette — ricade sull'infinito,
  // che è il comportamento di prima: fermo invece che sbagliato.
  const durata =
    brano.durationMs > 0 ? brano.durationMs : Number.MAX_SAFE_INTEGER;

  // L'avanzamento non passa da React: si scrive sul nodo e basta.
  useEffect(() => {
    const nodo = contenitore.current;
    if (!nodo) return;
    const fine = righe[attiva + 1]?.ms ?? durata;
    nodo.style.setProperty(
      "--avanzamento",
      String(avanzamento(righe, attiva, corretta, fine)),
    );

    // E se la riga accesa porta i tempi delle parole, ognuna riceve il suo.
    // Il ciclo gira sui figli diretti invece di interrogare il documento: sono
    // già solo le parole — quando non ce ne sono, la riga ha un nodo di testo e
    // nessun elemento, e questo ciclo non fa nemmeno un giro.
    const parole = righe[attiva]?.parole ?? [];
    const acceso = rigaAccesa.current;
    if (parole.length === 0 || !acceso) return;
    const quale = parolaAttiva(parole, corretta);
    for (let i = 0; i < acceso.children.length; i += 1) {
      const parola = acceso.children[i];
      if (!(parola instanceof HTMLElement)) continue;
      // Quel che è già stato detto è pieno, quel che deve ancora venire è
      // vuoto: il valore di mezzo ce l'ha una parola sola per volta. Le altre
      // ricevono lo stesso numero che avevano già, e riscrivere una proprietà
      // con il valore che porta di suo non fa ridipingere niente.
      const quanto =
        i < quale
          ? 1
          : i > quale
            ? 0
            : avanzamentoParola(parole, i, corretta, fine);
      parola.style.setProperty("--avanzamento", String(quanto));
      // E lo stato che le skin cercano, sulla parola in bocca. Scritto qui e
      // non da React per la ragione di tutto questo effetto: cambia dieci volte
      // per riga, e dieci ricostruzioni d'albero per riga sono trecento per
      // canzone in cambio di un attributo.
      if (i === quale) parola.dataset["active"] = "";
      else delete parola.dataset["active"];
    }
  }, [righe, attiva, corretta, durata]);

  /* Quando la riga accesa cambia, quella di prima spegne le sue parole.
     Serve perché quei valori non li mette React: React riusa gli stessi nodi
     — la chiave di una parola è stabile — e non sa di dover ripulire una
     proprietà che non ha scritto lui. Senza questo, ogni riga già cantata
     resterebbe con l'ultima parola marcata «attiva», e una skin che ridipinge
     `lyric-word` nello stato attivo accenderebbe una parola per strofa.

     Sta in un effetto suo, con `attiva` come sola dipendenza, perché è quel che
     lo fa girare qualche decina di volte per canzone invece di venti volte al
     secondo: il ripulimento è nella funzione di ritorno, e quella si chiama
     solo quando la dipendenza cambia davvero. */
  useEffect(() => {
    const acceso = rigaAccesa.current;
    return () => {
      if (!acceso) return;
      for (let i = 0; i < acceso.children.length; i += 1) {
        const parola = acceso.children[i];
        if (!(parola instanceof HTMLElement)) continue;
        parola.style.removeProperty("--avanzamento");
        delete parola.dataset["active"];
      }
    };
  }, [attiva]);

  /* Porta la riga accesa all'ancora.
     `scrollTo` sul contenitore e non `scrollIntoView` sul nodo: il secondo
     scorre **ogni** antenato scorrevole, cioè promette di muovere solo questo
     pannello e non lo garantisce. Il primo dice dove, e dice dove soltanto qui.
     È anche il motivo per cui `.righe-testo` è `position: relative`: così è lei
     l'`offsetParent`, e `offsetTop` è già la misura giusta. */
  const inseguiOra = useCallback((secco: boolean) => {
    const box = contenitore.current;
    const nodo = rigaAccesa.current;
    if (!box || !nodo) return;
    box.scrollTo({
      top: nodo.offsetTop - box.clientHeight * ANCORA,
      behavior: secco || fermoRestando() ? "auto" : "smooth",
    });
  }, []);

  // Lo scorrimento segue la riga accesa, a meno che non l'abbia mossa qualcuno.
  useEffect(() => {
    const prima = attivaPrima.current;
    attivaPrima.current = attiva;
    if (Date.now() < fermoFino.current) return;
    inseguiOra(prima < 0 || Math.abs(attiva - prima) > SALTO_SECCO);
  }, [attiva, inseguiOra]);

  /* Il gesto, non il suo effetto.
     Qui stava `onScroll`, e `onScroll` lo emette anche lo scorrimento che
     abbiamo appena chiesto noi: il pannello si metteva in pausa da solo a ogni
     inseguimento, e nel tempo della pausa saltava tutte le righe che passavano.
     Con una riga ogni due secondi non inseguiva più niente.

     Questi invece li fa solo una persona: la rotella e il dito. Nessuno dei due
     lo emette il browser per conto suo, ed è esattamente la differenza che
     serviva.

     Il trascinamento della barra non c'è perché la barra non c'è:
     `scrollbar-width: none`, poco sopra nel foglio. E `pointerdown` da solo
     sarebbe peggio di niente — scatta anche quando si clicca una riga per
     saltarci, cioè metterebbe in pausa l'inseguimento nel gesto che serve a
     riprenderlo. */
  const hoScorrutoIo = useCallback(() => {
    fermoFino.current = Date.now() + PAUSA_SCORRIMENTO;
    setInPausa(true);
    window.clearTimeout(scadenzaPausa.current);
    scadenzaPausa.current = window.setTimeout(() => {
      setInPausa(false);
    }, PAUSA_SCORRIMENTO);
  }, []);

  const riprendi = useCallback(() => {
    fermoFino.current = 0;
    window.clearTimeout(scadenzaPausa.current);
    setInPausa(false);
    inseguiOra(false);
  }, [inseguiOra]);

  /* Salta al tempo di una riga.
     Gli scarti si **disfano** invece di essere applicati: `posizione_corretta`
     somma offset e scarto alla posizione del lettore per confrontarla con i
     tempi, quindi per far cadere la riga esattamente qui la posizione da
     chiedere è il suo tempo meno quei due. Toccare i tempi delle righe sarebbe
     l'altra strada, ed è quella che tutto questo modulo non prende mai. */
  const saltaA = useCallback(
    (ms: number) => {
      if (!testo) return;
      const dove = Math.max(0, ms - testo.offsetMs - testo.scartoMs);
      // Chi salta vuole tornare a seguire: la pausa che aveva chiesto scorrendo
      // riguardava il punto in cui stava, e quel punto adesso è questo.
      fermoFino.current = 0;
      window.clearTimeout(scadenzaPausa.current);
      setInPausa(false);
      ipc.vaiA(dove).catch(onErrore);
    },
    [testo, onErrore],
  );

  const spostaDi = useCallback(
    (quanto: number) => {
      if (!testo) return;
      const nuovo = testo.scartoMs + quanto;
      setTesto({ ...testo, scartoMs: nuovo });
      ipc.testoScarto(brano.id, nuovo).catch(onErrore);
    },
    [brano.id, testo, onErrore],
  );

  // Da dove parte l'editor: il testo piatto se c'è, altrimenti le righe già
  // sincronizzate — chi risincronizza un LRC di un'altra edizione ha il testo
  // giusto e i tempi sbagliati, e ribattere le parole sarebbe lavoro rifatto.
  const daCuiPartire =
    testo?.piatto ?? righe.map((riga) => riga.testo).join("\n");

  /* Il bottone c'è sempre, e cambia parola: «sincronizza» quando non ci sono
     tempi, «risincronizza» quando ci sono. Nasconderlo nel secondo caso
     vorrebbe dire che un testo scaricato storto non si può raddrizzare senza
     prima cancellarlo — cioè che la fonte peggiore vince su di te. */
  const bottoneSincronizza = testo !== null && !testo.strumentale && (
    <button
      type="button"
      className="bottone minuto btn-ghost"
      title={t("np.lyrics.sync.hint")}
      onClick={() => setEditor(true)}
    >
      {righe.length > 0 ? t("np.lyrics.resync") : t("np.lyrics.sync")}
    </button>
  );

  /* La riga dei ripieghi: quel che si può fare quando il testo non c'è, o
     c'è ma senza tempi.

     Sta in una variabile e non scritta due volte perché la mostrano due rami
     diversi — il pannello vuoto e il testo piatto — e sono lo stesso gesto: in
     tutt'e due i casi il catalogo potrebbe avere i tempi e non glieli si è
     chiesti abbastanza. Fra i due bottoni cambia solo la parola: «Riprova»
     quando la rete è caduta, «Cerca di nuovo» quando ha risposto e non aveva
     niente. Sono due frasi perché sono due situazioni, e chiamarle allo stesso
     modo direbbe a chi ha il wifi staccato che il catalogo non conosce la sua
     canzone.

     Mentre si cerca non compaiono: un bottone «Riprova» accanto a «Cerco il
     testo…» offre di rifare quel che si sta già facendo. */
  const scelte = !cercando && (
    <div className="scelte-testo">
      <button
        type="button"
        className="bottone minuto btn-ghost"
        onClick={() => void chiedi(brano.id, true)}
      >
        {guasto ? t("np.lyrics.retry") : t("np.lyrics.again")}
      </button>
      {/* Qui il bottone è la risposta alla domanda che la schermata pone, non
          un comando accessorio: è l'unica via che resta, ed è quella che
          chiude la copertura fino in fondo. */}
      <button
        type="button"
        className="bottone primario btn-accent"
        onClick={() => setEditor(true)}
      >
        {t("np.lyrics.sync")}
      </button>
    </div>
  );

  /* L'introduzione, quando è lunga abbastanza da essere un'attesa.
     Sta prima dell'elenco e non dentro, perché nell'LRC non è una riga: è il
     tempo che c'è prima della prima. Porta `rigaAccesa` come la porterebbe una
     riga, e così l'inseguimento la trova senza sapere che è diversa. */
  const introduzione =
    attiva < 0 && righe.length > 0 && (righe[0]?.ms ?? 0) >= STACCO_MINIMO ? (
      <p
        ref={segna}
        className="lyric-line"
        data-attiva
        data-active
        data-stacco
      >
        <Stacco />
      </p>
    ) : null;

  const elenco = useMemo(
    () =>
      righe.map((riga, indice) => {
        const vuota = riga.testo.trim() === "";
        // Una riga vuota nell'LRC è una pausa fra due strofe, e ha un suo
        // tempo: si tiene, larga quanto una riga, invece di collassare. Quando
        // la pausa è quella in corso ed è lunga, al posto del vuoto ci sono i
        // puntini — c'è qualcosa da guardare proprio quando non c'è niente da
        // leggere.
        const stacco =
          vuota &&
          indice === attiva &&
          (righe[indice + 1]?.ms ?? durata) - riga.ms >= STACCO_MINIMO;

        /* Gli attributi che una riga porta comunque, pausa o no. Un oggetto
           solo e non due elenchi affiancati: sono sette, e ne basta uno
           dimenticato da una parte perché una skin ridipinga metà delle
           righe.

           `data-active` accanto a `data-attiva`, e non al posto suo: il
           registro delle parti compila lo stato «attivo» in
           `[data-active], .is-active` (`aether_skin::PartState::selector`),
           quindi senza questo attributo una skin può ridipingere `lyric-line`
           ma non la riga che sta suonando — cioè l'unica che si guarda. È
           lo stesso accoppiamento che `.riga` tiene con `aria-current`. */
        const comuni = {
          ref: indice === attiva ? segna : undefined,
          className: "lyric-line",
          "data-attiva": indice === attiva || undefined,
          "data-active": indice === attiva || undefined,
          "aria-current": indice === attiva || undefined,
          "data-passata": indice < attiva || undefined,
          "data-parole": riga.parole.length > 0 || undefined,
          "data-stacco": stacco || undefined,
        };

        /* Una pausa non è un bottone. Ci si potrebbe saltare — ha un tempo
           come tutte le altre — ma un bottone senza parole dentro è un bottone
           senza nome, e in un testo lungo le pause sono decine: decine di
           fermate anonime per chi attraversa la schermata con lo screen reader,
           in cambio di un salto in un punto in cui non si canta. */
        if (vuota) {
          return (
            <p key={`${riga.ms}-${indice}`} {...comuni}>
              {/* Uno spazio unificatore: un paragrafo vuoto non ha altezza, e
                  la pausa nell'LRC è larga quanto una riga. */}
              {stacco ? <Stacco /> : " "}
            </p>
          );
        }

        return (
          <button
            key={`${riga.ms}-${indice}`}
            type="button"
            {...comuni}
            /* Una sola di duecento righe entra nell'ordine di tabulazione.
               Senza, attraversare questa schermata col tasto Tab vorrebbe dire
               duecento fermate dentro un pannello che si legge e basta. Con il
               fuoco sulla riga accesa le frecce scorrono comunque il
               contenitore, che è quel che serviva davvero. */
            tabIndex={indice === attiva ? 0 : -1}
            title={t("np.lyrics.seek.hint")}
            onClick={() => saltaA(riga.ms)}
          >
            {riga.parole.length > 0 ? (
              /* Un elemento per parola, con dentro i suoi spazi: il testo
                 resta parola per parola quello dell'LRC, e a capo va dove
                 andrebbe comunque. La chiave porta il tempo perché in una
                 canzone la stessa parola torna, e l'indice da solo
                 rimescolerebbe i nodi a ogni ritornello.

                 Niente involucro qui sotto: ogni parola è già un elemento
                 inline con il suo avanzamento, quindi l'ordine è già giusto
                 quando la riga va a capo — ed è anche quel che l'effetto si
                 aspetta di trovare fra i figli diretti della riga. */
              riga.parole.map((parola, quale) => (
                <span key={`${parola.ms}-${quale}`} className="lyric-word">
                  {parola.testo}
                </span>
              ))
            ) : (
              /* L'involucro che porta l'illuminazione, e la ragione per cui
                 c'è.

                 Il fronte era un gradiente sul **blocco**, e un blocco che va a
                 capo ha una sola larghezza: lo stesso taglio orizzontale
                 cadeva su tutte e due le righe visive insieme. Su «Guardo nel
                 retrovisore, dietro me si sta / scuencendo l'autostrada» a un
                 quarto della riga si accendevano «Guardo nel» **e**
                 «scuencendo» — due frammenti staccati, e la seconda riga che
                 si illuminava in parallelo alla prima invece che dopo.

                 Su un elemento **inline** che va a capo, invece,
                 `box-decoration-break: slice` — che è il valore di partenza —
                 dice di dipingere il fondo come se i frammenti fossero in fila
                 e poi affettarlo. Cioè esattamente l'ordine giusto: il fronte
                 finisce la prima riga e poi comincia la seconda. */
              <span className="lyric-fill">{riga.testo}</span>
            )}
          </button>
        );
      }),
    [righe, attiva, durata, saltaA, segna],
  );

  return (
    <aside
      className="testo-np lyrics-screen"
      // L'ancora del giro guidato: il ripiego della schermata a tutto schermo,
      // per gli scafali che mostrano il testo senza di lei.
      data-giro="testo"
      aria-label={t("np.lyrics")}
    >
      <header>
        <span className="occhiello hero-eyebrow">{t("np.lyrics")}</span>
        {/* La chiave si compone a runtime dal nome della fonte, quindi passa da
            `tSe` con il suo ripiego obbligatorio: TypeScript non può verificare
            una chiave calcolata, e fingere il contrario con un cast renderebbe
            questo caso indistinguibile da quelli veri. */}
        {testo && testo.fonte !== "nessuna" && (
          <span className="fonte-testo" title={t("np.lyrics.source.hint")}>
            {tSe(`np.lyrics.source.${testo.fonte}`, testo.fonte)}
          </span>
        )}
        {bottoneSincronizza}
      </header>

      {/* La striscia della correzione c'è ogni volta che c'è qualcosa da
          correggere, cioè ogni volta che ci sono righe con un tempo.

          Qui stava scritto che «sempre visibile sarebbe un comando in cerca di
          un problema», e la striscia compariva solo con l'aderenza diversa da
          «buona». Era falso, e il falso costava una funzione. L'aderenza misura
          una cosa sola: se i tempi **stanno dentro** la durata di questo file —
          è `aether_domain::testo::verifica_durata`, e guarda dove finisce
          l'ultima riga. Non dice niente su dove cadano. Un `.lrc` battuto su
          un'altra edizione con la stessa lunghezza, un master con mezzo secondo
          di silenzio in testa, una catena d'uscita che ritarda più di quanto il
          motore sappia misurare: tutti e tre danno un testo sfasato di un tanto
          costante e un'aderenza «buona», e tutti e tre si raddrizzano con questi
          due bottoni. Che erano irraggiungibili proprio nel caso più comune.

          Quel che dipende ancora dall'aderenza è la **frase**: dire perché la
          striscia è lì serve quando c'è qualcosa da segnalare, e quando non c'è
          niente da segnalare la striscia si fa da parte — `discreta`. */}
      {testo && righe.length > 0 && (
        <div
          className={
            testo.aderenza === "buona"
              ? "scarto-testo discreta"
              : "scarto-testo"
          }
          role="group"
          aria-label={t("np.lyrics.nudge")}
        >
          {testo.aderenza !== "buona" && (
            <span className="perche">
              {tSe(`np.lyrics.check.${testo.aderenza}`, t("np.lyrics.check"))}
            </span>
          )}
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={() => spostaDi(PASSO_SCARTO)}
            title={t("np.lyrics.nudge.earlier")}
          >
            −{PASSO_SCARTO}
          </button>
          <span className="quanto">{t("np.lyrics.nudge.value", { ms: testo.scartoMs })}</span>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={() => spostaDi(-PASSO_SCARTO)}
            title={t("np.lyrics.nudge.later")}
          >
            +{PASSO_SCARTO}
          </button>
        </div>
      )}

      <div
        className="righe-testo"
        ref={contenitore}
        /* Il respiro sopra e sotto — quel che permette alla prima riga di
           salire all'ancora e all'ultima di scendere — è un paio di
           distanziatori nel foglio di stile, e sono appesi a questo attributo:
           un testo senza tempi non insegue niente, e mezzo pannello di vuoto
           sopra di lui sarebbe solo un pannello che sembra vuoto. */
        data-scorre={righe.length > 0 || undefined}
        /* Un'area che scorre e non si può scorrere da tastiera è un'area che
           una parte delle persone non legge oltre la prima schermata. */
        tabIndex={0}
        role="group"
        aria-label={t("np.lyrics")}
        onWheel={hoScorrutoIo}
        onTouchMove={hoScorrutoIo}
        onKeyDown={(e) => {
          if (TASTI_CHE_SCORRONO.has(e.key)) hoScorrutoIo();
        }}
      >
        {testo === null ? null : testo.strumentale ? (
          <div className="niente-testo">
            <Icona nome="i-eq" dim={20} />
            <p>{t("np.lyrics.instrumental")}</p>
            <span>{t("np.lyrics.instrumental.hint")}</span>
          </div>
        ) : righe.length > 0 ? (
          <>
            {introduzione}
            {elenco}
          </>
        ) : testo.piatto ? (
          /* Senza tempi si mostra il testo e basta, **senza** evidenziazione:
             illuminare una riga a caso perché il tempo è passato sarebbe
             inventare una sincronia che nessuno ha misurato, e uno che legge
             non ha modo di sapere che è inventata. E senza clic: un tempo a cui
             saltare non c'è. */
          <div className="testo-piatto">
            <span className="occhiello">{t("np.lyrics.plain")}</span>
            {testo.piatto.split("\n").map((riga, indice) => (
              <p key={`${indice}-${riga}`} className="lyric-line">
                {riga.trim() === "" ? " " : riga}
              </p>
            ))}
            {/* Le parole ci sono, i tempi no: è il caso in cui il catalogo
                può ancora avere qualcosa, perché tiene più voci per brano e
                quella che ha risposto non è sempre quella con i tempi. */}
            {scelte}
          </div>
        ) : (
          <div className="niente-testo">
            <Icona nome="i-text" dim={20} />
            <p>
              {cercando
                ? t("np.lyrics.searching")
                : guasto
                  ? t("np.lyrics.failed")
                  : t("np.lyrics.none")}
            </p>
            {!cercando && guasto && <span>{t("np.lyrics.failed.hint")}</span>}
            {!cercando && !guasto && testo.cercato && (
              <span>{t("np.lyrics.none.hint")}</span>
            )}
            {scelte}
          </div>
        )}
      </div>

      {/* La pausa dello scorrimento, resa visibile.
          Sta sopra il pannello e non in colonna con lui: comparendo in flusso
          sposterebbe di venti pixel tutto il testo, cioè userebbe il movimento
          per dire che qualcosa **non** si sta muovendo. */}
      {inPausa && righe.length > 0 && (
        <button
          type="button"
          className="torna-al-brano bottone minuto btn-ghost"
          onClick={riprendi}
        >
          <Icona nome="i-chev-d" dim={12} />
          {t("np.lyrics.resume")}
        </button>
      )}

      {/* Montata da qui e non da `App`: la modale disegna un velo in posizione
          fissa, quindi il posto nell'albero non cambia dove finisce sullo
          schermo — e tenerla qui evita di far scendere quattro prop attraverso
          `InRiproduzione` per una schermata che apre solo questo pannello. */}
      {editor && testo !== null && (
        <Sincronizza
          brano={brano}
          iniziale={daCuiPartire}
          /* Solo le parole scendono: nessuno dei due scarti attraversa
             l'editor. Le battute che ne escono sono misurate sulla posizione
             grezza, e `testo_salva` spiega per esteso perché portarsi dietro un
             offset le sfaserebbe tutte. */
          onChiudi={() => setEditor(false)}
          onSalvato={setTesto}
          onErrore={onErrore}
        />
      )}
    </aside>
  );
}
