/**
 * Un elenco che arriva a pagine.
 *
 * # Perché esisteva un tetto, e perché non poteva restare
 *
 * La libreria si chiedeva una volta sola e con un limite: duecento brani,
 * quattrocento album, e per i preferiti duemila righe filtrate nella finestra.
 * Su una libreria da duecentocinquanta brani — quella vera di chi ci lavora —
 * il brano duecentouno non era raggiungibile **da nessuna vista**: non c'era un
 * tasto, non c'era uno scorrimento, non c'era un messaggio. Semplicemente
 * l'elenco finiva.
 *
 * # Scorrimento e non pagine numerate
 *
 * Una libreria musicale si scorre. «Pagina 2» chiede di sapere dove sta una
 * cosa prima di cercarla, che è esattamente quel che non si sa: la pagina
 * successiva si chiede da sé quando il fondo si avvicina.
 *
 * # Il giro
 *
 * Ogni azzeramento incrementa `giro`. Una risposta partita per la chiave di
 * prima — si cambia vista mentre una pagina è in volo — arriva con un giro
 * vecchio e viene buttata invece di accodarsi a un elenco che non è più il suo.
 * È lo stesso motivo per cui la ricerca aveva già un `annullato`.
 */
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/**
 * Quante righe per pagina.
 *
 * Duecento è quel che il limite era già: abbastanza perché su una libreria
 * normale la seconda pagina non si chieda mai, poco abbastanza perché la prima
 * arrivi subito.
 */
export const PAGINA = 200;

/** Un elenco impaginato, e i modi per muoverlo. */
export interface Pagine<T> {
  /** Le righe arrivate finora, in ordine. */
  righe: T[];
  /** La **prima** pagina non è ancora arrivata. */
  caricando: boolean;
  /** Ce n'è ancora, e la sentinella la chiederà. */
  altre: boolean;
  /** Va appeso a un elemento in fondo all'elenco. */
  sentinella: (nodo: HTMLDivElement | null) => (() => void) | undefined;
  /** Butta tutto e richiede dalla prima pagina. */
  ricarica: () => void;
  /**
   * Riscrive le righe già in mano.
   *
   * Serve agli aggiornamenti ottimistici — cuore e stelle rispondono al dito e
   * non al disco — e a niente altro: non chiede niente al nucleo.
   */
  aggiorna: (f: (righe: T[]) => T[]) => void;
}

/**
 * Segue un elenco a pagine.
 *
 * `chiave` è l'identità di **quale** elenco si sta guardando: cambiarla butta
 * quel che c'è e ricomincia da capo. `chiedi` può essere una lambda scritta sul
 * posto — la sua identità non entra in nessuna dipendenza.
 */
export function usePagine<T>(
  chiedi: (offset: number, limite: number) => Promise<T[]>,
  chiave: string,
  onErrore: (e: unknown) => void,
  misura: number = PAGINA,
): Pagine<T> {
  const [righe, setRighe] = useState<T[]>([]);
  const [caricando, setCaricando] = useState(true);
  const [altre, setAltre] = useState(false);

  // In un ref e non fra le dipendenze: quasi sempre arrivano come lambda
  // scritte al punto di chiamata, e la loro identità cambia a ogni disegno.
  // Metterle nelle dipendenze farebbe ricominciare l'elenco sessanta volte al
  // secondo; chiedere a chi chiama di avvolgerle in `useCallback` sposterebbe
  // su di lui un vincolo che è di qui.
  const chiediRef = useRef(chiedi);
  chiediRef.current = chiedi;
  const onErroreRef = useRef(onErrore);
  onErroreRef.current = onErrore;

  const giro = useRef(0);
  const inVolo = useRef(false);
  const finite = useRef(false);
  /** Quante righe abbiamo, senza doverle leggere dallo stato. */
  const quante = useRef(0);

  const chiediPagina = useCallback(
    (offset: number) => {
      if (inVolo.current) return;
      inVolo.current = true;
      const mio = giro.current;
      if (offset === 0) setCaricando(true);
      chiediRef
        .current(offset, misura)
        .then((arrivate) => {
          if (mio !== giro.current) return;
          quante.current = offset === 0 ? arrivate.length : quante.current + arrivate.length;
          setRighe((prima) => (offset === 0 ? arrivate : [...prima, ...arrivate]));
          // Una pagina più corta della misura è l'ultima: chiederne un'altra
          // sarebbe un giro per ricevere zero righe.
          const ancora = arrivate.length === misura;
          finite.current = !ancora;
          setAltre(ancora);
        })
        .catch((e: unknown) => {
          if (mio !== giro.current) return;
          onErroreRef.current(e);
          // Fermarsi invece di riprovare da sé: la sentinella è ancora in
          // vista, e senza questo un guasto diventerebbe un anello di richieste
          // che falliscono.
          finite.current = true;
          setAltre(false);
        })
        .finally(() => {
          // Solo se il giro è ancora il nostro: se è cambiato, la richiesta
          // nuova ha già alzato `inVolo` e abbassarlo qui ne permetterebbe una
          // seconda in parallelo.
          if (mio !== giro.current) return;
          inVolo.current = false;
          setCaricando(false);
        });
    },
    [misura],
  );

  const ricarica = useCallback(() => {
    giro.current += 1;
    inVolo.current = false;
    finite.current = false;
    quante.current = 0;
    setRighe([]);
    setAltre(false);
    chiediPagina(0);
  }, [chiediPagina]);

  useEffect(ricarica, [chiave, ricarica]);

  const sentinella = useCallback(
    (nodo: HTMLDivElement | null) => {
      if (nodo === null) return undefined;
      const osservatore = new IntersectionObserver(
        (voci) => {
          // Il riflesso della barra scorre solo a sentinella vicina. Lontana,
          // un'animazione di sfondo infinita costa un ridisegno a ogni
          // fotogramma anche dove nessuno la vede: misurato, quindici-venti
          // per cento di un core con la musica ferma e l'elenco degli album
          // aperto. La regola che la ferma sta in `stile.css`, accanto a
          // `.sentinella .skeleton`; qui si dice solo dov'è.
          const vicina = voci.some((v) => v.isIntersecting);
          if (vicina) nodo.dataset.vicina = "";
          else delete nodo.dataset.vicina;
          if (!vicina) return;
          if (finite.current || inVolo.current) return;
          chiediPagina(quante.current);
        },
        // Un margine largo: la pagina si chiede **prima** che il fondo si
        // veda, così chi scorre non si ferma ad aspettarla.
        { rootMargin: "600px" },
      );
      osservatore.observe(nodo);
      return () => osservatore.disconnect();
    },
    [chiediPagina],
  );

  const aggiorna = useCallback((f: (righe: T[]) => T[]) => setRighe(f), []);

  // Memoizzato: l'oggetto finisce nelle dipendenze di chi lo usa, e
  // un'identità nuova a ogni disegno rifarebbe quegli effetti sessanta volte al
  // secondo. `ricarica` e `aggiorna` sono già stabili di loro.
  return useMemo(
    () => ({ righe, caricando, altre, sentinella, ricarica, aggiorna }),
    [righe, caricando, altre, sentinella, ricarica, aggiorna],
  );
}

/**
 * Un valore che segue l'originale con un ritardo.
 *
 * Digitando «subsonica» sarebbero nove ricerche, di cui otto già superate
 * quando tornano. Stava dentro l'effetto della ricerca; qui è un valore, e
 * quindi può diventare parte della **chiave** di un elenco impaginato invece
 * che di una richiesta scritta a mano.
 */
export function usePigro<T>(valore: T, ritardoMs: number): T {
  const [pigro, setPigro] = useState(valore);
  useEffect(() => {
    const attesa = setTimeout(() => setPigro(valore), ritardoMs);
    return () => clearTimeout(attesa);
  }, [valore, ritardoMs]);
  return pigro;
}

/**
 * Ascolta un evento del nucleo per tutta la vita del componente.
 *
 * # Perché non basta `useEffect` con `listen` dentro
 *
 * Per due ragioni, e tutte e due si sono viste in `App.tsx`.
 *
 * La prima è che `listen` restituisce una **promessa**, e la pulizia di un
 * effetto è sincrona: `promessa.then((stop) => stop())` scioglie l'ascolto
 * quando la promessa si risolve, che può essere dopo che l'effetto è già
 * ripartito. Nel mezzo gli ascoltatori vivi sono due, e l'evento arriva a tutti
 * e due. Per `tauri://drag-drop` questo voleva dire che una skin trascinata
 * nella finestra si installava **due volte**.
 *
 * La seconda è che il gestore quasi sempre legge lo stato di adesso, e metterlo
 * nelle dipendenze fa registrare e sciogliere l'ascolto a ogni cambio di quello
 * stato — che è insieme lo spreco e il modo di innescare la prima ragione.
 *
 * Qui il gestore sta dietro un riferimento tenuto aggiornato a ogni disegno:
 * l'ascolto si apre una volta, e chi lo riceve è sempre l'ultima versione. La
 * pulizia ricorda se il componente è ancora vivo, così una promessa che si
 * risolve in ritardo scioglie subito invece di lasciare un ascoltatore orfano.
 *
 * `evento` deve essere costante: è la sola dipendenza, ed è il nome di un
 * canale, non un valore.
 *
 * # `acceso`, e perché non basta non chiamare l'hook
 *
 * Chi non vuole l'ascolto lo dice con `acceso = false` invece di saltare la
 * chiamata: gli hook si contano per posizione, e un `useAscolto` dietro un `if`
 * cambierebbe l'ordine fra un disegno e l'altro — che React tratta, a ragione,
 * come un errore duro. Qui l'hook si chiama sempre, e a spegnersi è l'effetto,
 * con un'uscita anticipata e `acceso` fra le dipendenze: passando a `false`
 * l'ascolto si scioglie, tornando a `true` si riapre.
 *
 * Serve allo spettro con la sorgente sintetica (`Spettro3D`, `sorgente:
 * "finto"`), che le sue file se le fabbrica e non deve né ricevere né chiedere
 * quelle del motore.
 */
export function useAscolto<T>(
  evento: string,
  gestore: (carico: T) => void,
  acceso = true,
): void {
  const ultimo = useRef(gestore);
  // Senza array di dipendenze: gira dopo ogni disegno, che è precisamente
  // quando `gestore` può essere cambiato.
  useEffect(() => {
    ultimo.current = gestore;
  });
  useEffect(() => {
    if (!acceso) return;
    let vivo = true;
    let sciogli: (() => void) | null = null;
    void listen<T>(evento, (arrivato) => ultimo.current(arrivato.payload)).then((stop) => {
      if (vivo) {
        sciogli = stop;
      } else {
        // La promessa si è risolta dopo lo smontaggio: l'ascoltatore esiste già
        // ed è orfano. Si scioglie qui, che è l'unico posto in cui lo si ha in
        // mano.
        stop();
      }
    });
    return () => {
      vivo = false;
      sciogli?.();
    };
  }, [evento, acceso]);
}
