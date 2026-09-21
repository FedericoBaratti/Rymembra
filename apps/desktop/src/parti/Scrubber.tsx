/**
 * Il cursore della posizione nel brano.
 *
 * # Perché lo stato del trascinamento sta qui
 *
 * È l'unico stato di questa interfaccia che il nucleo non conosce, ed è giusto
 * che non lo conosca: una posizione *voluta* e non ancora avvenuta. Finché il
 * dito è giù, `aether-play` sta ancora suonando dov'era, e chi trascina deve
 * vedere dove sta andando invece di dove è.
 *
 * Stava scritto in tre componenti — la barra, la colonna e lo schermo intero —
 * insieme alle stesse quattordici righe di `<input type="range">` e alla stessa
 * `rilascia()`. Tre copie di uno stato sono tre occasioni di correggerne due.
 *
 * # Perché la posizione non è una prop
 *
 * Perché questo è **l'unico** componente che la disegna. Scendeva da `App`
 * attraverso `contesto`, `Impaginazione`, `Lettore` e `Colonna` — cinque
 * livelli che non ne facevano niente — e cambiando venti volte al secondo li
 * faceva ridisegnare tutti, elenco dei brani compreso. Letta qui, il suo
 * cambiamento arriva a un componente solo.
 *
 * # Il salto che dura, e perché lo si dice
 *
 * Su un brano di catalogo spostarsi non è gratis. Misurato su un mp3 da venti
 * megabyte preso dall'Internet Archive: saltare a metà ha richiesto un minuto,
 * e non per un guasto — symphonia, su un mp3 senza tavola di salto, *scorre*
 * fino al punto, e scorrere vuol dire chiedere alla rete tutti i byte in mezzo.
 * La finestra, nel frattempo, diceva «in riproduzione» con la posizione ferma:
 * nessun errore, nessun segnale, niente da capire.
 *
 * Quell'attesa è più corta di prima — la finestra di `aether-net` adesso cresce
 * mentre si legge di fila, e il nodo dell'Archive si risolve una volta sola —
 * ma resta un'attesa, perché i byte in mezzo vanno comunque scaricati. Quel che
 * non deve restare è il silenzio: finché il motore non è arrivato, il cursore
 * lo dice.
 */
import { useEffect, useState, type CSSProperties } from "react";

import { durata } from "../formato";
import { ipc, type StatoRiproduzione } from "../ipc";
import { usePosizioneMs } from "../riproduzione";
import { t } from "../lingue";

/**
 * Quanto vicino al bersaglio vuol dire «ci siamo».
 *
 * Tre secondi. Il motore non atterra mai esattamente sul millisecondo chiesto —
 * su un mp3 si ferma al fotogramma buono più vicino — e una tolleranza stretta
 * lascerebbe accesa per sempre la scritta di un salto perfettamente riuscito.
 * Larga, al contrario, non costa niente: chi ha appena trascinato il cursore
 * non conta i secondi, guarda se la musica riparte.
 */
const TOLLERANZA_MS = 3000;

/**
 * Quanto si aspetta prima di credere alla posizione.
 *
 * `vai_a` annuncia il punto **richiesto** un istante prima che il motore ci
 * vada — è `manda_stato_con_posizione`, e serve a non far rimbalzare il cursore
 * sul punto di partenza. Per un decimo di secondo, quindi, la posizione mostrata
 * è già quella d'arrivo anche quando il motore non si è mosso, e un confronto
 * fatto lì dichiarerebbe finito ogni salto nell'istante in cui comincia.
 *
 * Un secondo e mezzo è il tempo in cui il primo colpo d'orologio vero arriva e
 * rimette la posizione dov'è davvero. Su un salto che finisce subito non si
 * perde niente — la scritta compare e sparisce, che è la verità — e su uno che
 * dura è il ritardo con cui comincia a dirlo.
 */
const PRIMA_DI_CREDERCI_MS = 1500;

/**
 * Dopo quanto si smette di dire «sto cercando il punto».
 *
 * # Il difetto che chiude
 *
 * L'attesa si spegneva a una condizione sola: la posizione che torna vicina al
 * bersaglio. È una condizione che si valuta **quando la posizione cambia**, e
 * se il salto fallisce e la riproduzione si ferma la posizione non cambia più:
 * l'effetto non si rivaluta, e la scritta resta accesa finché non si cambia
 * brano. A tempo indeterminato, su un brano fermo, con scritto «sto cercando».
 *
 * # Perché un minuto
 *
 * Perché un salto lungo dentro un MP3 senza indice **dura davvero** decine di
 * secondi: il decodificatore ci arriva scorrendo, e scorrere vuol dire chiedere
 * alla rete tutti i byte in mezzo. Un tetto stretto trasformerebbe un'attesa
 * legittima in un falso allarme. Sessanta secondi sono oltre il caso peggiore
 * misurato, quindi quel che li supera è un salto che non arriverà.
 */
const SCADENZA_CERCA_MS = 60_000;

export function Scrubber({
  stato,
  conTempi = true,
  onErrore,
}: {
  stato: StatoRiproduzione;
  /** I due tempi ai lati. Toglierli lascia la sola barra. */
  conTempi?: boolean;
  onErrore: (e: unknown) => void;
}) {
  const posizioneMs = usePosizioneMs();
  const [trascinato, setTrascinato] = useState<number | null>(null);
  /**
   * Il punto a cui si è chiesto di andare, finché il motore non ci arriva.
   *
   * `null` quasi sempre: si riempie solo saltando dentro un brano di catalogo,
   * che è l'unico caso in cui l'arrivo non è immediato. Su un file resta
   * `null`, e questa parte di componente non esiste.
   *
   * Porta anche **quando** è stato chiesto, e non è un dettaglio contabile:
   * vedi [`PRIMA_DI_CREDERCI_MS`].
   */
  const [cercando, setCercando] = useState<{
    bersaglio: number;
    da: number;
    /** Il salto ha superato [`SCADENZA_CERCA_MS`] e non è arrivato. */
    scaduto: boolean;
  } | null>(null);
  const eUnFlusso = stato.brano?.fonte != null;
  // Sopra `rilascia` perché la legge anche lei, e non solo il disegno: il tetto
  // del salto e il tetto di quel che si vede sono lo stesso numero, e tenerli a
  // due righe di distanza è il modo di non correggerne uno solo.
  const durataMs = stato.durataMs;

  const rilascia = async () => {
    if (trascinato === null) return;
    try {
      // `vai_a` manda lo stato prima di rispondere, e con dentro i millisecondi
      // **richiesti**: il motore ci arriva sul filo suo poco dopo, ma quando
      // questa promessa si risolve la posizione nuova è già stata annunciata.
      // Togliere il trascinamento non fa quindi lampeggiare il cursore sul
      // punto di partenza — che è quel che succedeva finché lo stato portava
      // la posizione letta dal motore, cioè quella di prima del salto.
      // Con lo stesso tetto del numero *mostrato* qui sotto, e non con quello
      // grezzo: `max` è `durataMs`, quindi il cursore tirato fino in fondo
      // manda esattamente la durata dichiarata dal database. Su un flusso —
      // dove la durata vera la conosce solo il decodificatore — quel numero è
      // spesso oltre l'ultimo campione, e chiedere un salto oltre la fine è
      // chiedere la fine: il gesto «vai in fondo» diventava «brano successivo».
      const bersaglio = Math.min(trascinato, durataMs);
      await ipc.vaiA(bersaglio);
      // Solo su un flusso, e solo per un salto che non sia già quasi a posto:
      // sotto la tolleranza il motore ci arriva nel tempo di un fotogramma, e
      // accendere un'attesa per quello vorrebbe dire un lampeggio a ogni
      // ritocco del cursore.
      if (eUnFlusso && Math.abs(bersaglio - posizioneMs) > TOLLERANZA_MS) {
        setCercando({ bersaglio, da: performance.now(), scaduto: false });
      }
    } catch (e) {
      onErrore(e);
    }
    setTrascinato(null);
  };

  /*
   * L'attesa finisce quando il motore è arrivato, e non prima.
   *
   * Il confronto è con la posizione **vera**: `vai_a` annuncia in anticipo i
   * millisecondi richiesti, ma il primo colpo d'orologio che arriva davvero dal
   * motore riporta il punto di partenza — ed è lì che resta finché il salto non
   * è finito. È quindi il ritorno vicino al bersaglio a dire «ci siamo».
   *
   * Il brano che cambia chiude comunque l'attesa: saltare e poi premere
   * «successivo» non deve lasciare accesa la scritta di un salto che non
   * interessa più a nessuno.
   */
  const idBrano = stato.brano?.id ?? null;
  useEffect(() => {
    if (cercando === null) return;
    // Non prima che la posizione annunciata abbia lasciato il posto a quella
    // vera: vedi [`PRIMA_DI_CREDERCI_MS`].
    if (performance.now() - cercando.da < PRIMA_DI_CREDERCI_MS) return;
    if (Math.abs(posizioneMs - cercando.bersaglio) <= TOLLERANZA_MS) {
      setCercando(null);
    }
  }, [posizioneMs, cercando]);
  useEffect(() => setCercando(null), [idBrano]);

  /*
   * E se non arriva, a un certo punto lo si dice.
   *
   * L'effetto qui sopra dipende da `posizioneMs`: su un salto fallito quella
   * smette di arrivare, l'effetto non si rivaluta più e la scritta resterebbe
   * accesa per sempre. Questo è l'unico orologio della coppia che non dipende
   * dal motore, ed è quel che chiude il caso.
   *
   * Non cancella l'attesa: la **cambia di frase**. Sparire in silenzio
   * lascerebbe un cursore fermo senza spiegazione, che è precisamente lo stato
   * che la scritta era stata scritta per evitare.
   */
  useEffect(() => {
    if (cercando === null || cercando.scaduto) return;
    const orologio = window.setTimeout(() => {
      setCercando((c) => (c === null ? null : { ...c, scaduto: true }));
    }, SCADENZA_CERCA_MS);
    return () => window.clearTimeout(orologio);
  }, [cercando]);

  /*
   * Dove si sta andando batte dove si è. Vale per il dito sul cursore — è la
   * ragione per cui `trascinato` esiste — e vale identico per un salto che il
   * motore non ha ancora finito: il cursore che tornasse indietro al punto di
   * partenza, per un minuto, direbbe che il gesto non è stato raccolto.
   */
  const dove = trascinato ?? cercando?.bersaglio ?? posizioneMs;
  const avanzamento = durataMs > 0 ? (dove / durataMs) * 100 : 0;

  return (
    // L'ancora del giro guidato, che qui è un **ripiego**: nella barra è il
    // guscio dei comandi a portarla, e quello contiene già trasporto e cursore
    // insieme. Serve agli scafali che montano `scrubber` da solo, dove il
    // guscio non c'è: `Giro` prende il primo disegnato nell'ordine del
    // documento, quindi quando ci sono tutti e due vince il guscio.
    <div className="cursore player-progress" data-giro="trasporto">
      {conTempi && <span className="tempo">{durata(dove)}</span>}
      <input
        type="range"
        className="scorrimento range-accent"
        min={0}
        max={Math.max(durataMs, 1)}
        step={250}
        value={Math.min(dove, durataMs)}
        /* Il riempimento passa da una variabile invece che da un gradiente
           ricomposto a ogni fotogramma: è una proprietà custom sola, e il
           motore di rendering la risolve senza rileggere la regola. */
        style={{ "--avanzamento": `${avanzamento}%` } as CSSProperties}
        aria-label={t("player.position")}
        /* Senza questo uno screen reader legge il valore grezzo: «124500», che
           sono i millisecondi. È il cursore più usato dell'applicazione, e il
           numero che annunciava non era sbagliato — era illeggibile. Il tempo si
           scrive con `durata()`, la stessa funzione dei due tempi ai lati, così
           quel che si sente e quel che si vede non possono divergere; la durata
           totale ci sta dentro perché l'etichetta dice «posizione nel brano» e
           una posizione senza il suo fondo non si colloca. */
        aria-valuetext={t("player.position.value", {
          posizione: durata(dove),
          durata: durata(durataMs),
        })}
        disabled={durataMs === 0}
        onChange={(e) => setTrascinato(Number(e.target.value))}
        onPointerUp={() => void rilascia()}
        onKeyUp={() => void rilascia()}
        onBlur={() => void rilascia()}
      />
      {conTempi && <span className="tempo">{durata(durataMs)}</span>}
      {/* La scritta sta **dentro** il cursore e non in un angolo della barra:
          l'attesa appartiene al gesto che l'ha aperta, e una nota lontana dal
          cursore che si è appena mosso è una nota che nessuno collega. Il
          `title` dice anche perché, per chi si chiede se sia rotto. */}
      {cercando !== null && (
        <span
          className="tempo cercando"
          data-scaduto={cercando.scaduto ? "" : undefined}
          role="status"
          title={t(
            cercando.scaduto
              ? "player.seeking.failed.title"
              : "player.seeking.stream",
          )}
        >
          {t(cercando.scaduto ? "player.seeking.failed" : "player.seeking")}
        </span>
      )}
    </div>
  );
}
