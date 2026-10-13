/**
 * Come la finestra si presenta: tema, movimento, zoom, lingua, scorciatoie e
 * l'accento che segue la copertina.
 *
 * # Perché adesso sta qui
 *
 * `nuvola.ts` spiegava perché il tema era rimasto in `App`: tirava dentro
 * `avvio`, la skin attiva, il brano che suona e la variante chiara, e
 * l'effetto dell'accento ha un vettore di dipendenze che è il cuore della
 * cosa. Era vero, e resta vero: quel vettore è qui intero, scritto una volta
 * sola, e quel che arriva da fuori lo riceve come argomento — `avvio`, la skin
 * indossata, l'archivio del lettore, il segno dell'anteprima revocata.
 * Quattro valori e due funzioni, non la stessa riga scritta due volte.
 *
 * La copertina di quel che suona l'hook la segue da sé, dall'archivio
 * (`riproduzione.ts`), senza far ridisegnare niente: prima `App` si iscriveva
 * al lettore per passarla qui, e ogni brano nuovo rifaceva la finestra intera
 * per un colore. Vedi l'effetto dell'accento.
 *
 * In cambio, sei preferenze che si leggono, si applicano e si scrivono allo
 * stesso modo stanno vicine, e il corpo di `App` non le attraversa più per
 * arrivare all'instradamento.
 *
 * # L'ordine degli effetti
 *
 * L'hook si chiama nel punto di `App` in cui stavano i suoi effetti — dopo la
 * skin, prima della colonna che segue la larghezza — e li registra nello
 * stesso ordine in cui stavano: applicare il tema, applicare il movimento,
 * leggerlo, leggere lo zoom, decidere il tema, decidere la lingua, le voci del
 * vassoio, il sistema operativo che cambia tema, la preferenza dell'accento,
 * l'accento, la finestra che si mostra. Nessuno è un `useLayoutEffect`: il
 * primo fotogramma giusto lo garantisce l'attesa di `pronto`, non l'ordine.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { applicaAccento, applicaMovimento, type MovimentoUtente } from "./aspetto";
import { ipc, type Avvio, type Skin } from "./ipc";
import { applicaLingua, scegli, t, useLingua } from "./lingue";
import type { ArchivioLettore } from "./riproduzione";
import { leggiAssociazioni, scriviAssociazioni, type Associazioni } from "./tastiera";
import {
  applicaTema,
  dimenticaRipiego,
  seguiIlSistema,
  temaDiRipiego,
  type Tema,
} from "./tema";

/** Quel che `useAspetto` legge dal resto della finestra. */
export interface IngressiAspetto {
  /** Le preferenze salvate, o `null` finché non sono arrivate. */
  avvio: Avvio | null;
  /** La skin indossata: vedi `skinAttiva.ts`. */
  skinAttiva: Skin | null;
  /** Lo stato del lettore: la copertina di quel che suona si legge di lì. */
  lettore: ArchivioLettore;
  /** Il segno che un'anteprima di skin è stata revocata: vedi `skinAttiva.ts`. */
  accentoDaRimettere: number;
  segnalaErrore: (e: unknown) => void;
  /** Rilegge `avvio`: le scorciatoie si mostrano quando il database le ha prese. */
  ricarica: () => Promise<void>;
}

/** L'aspetto della finestra, e come si cambia. */
export interface Aspetto {
  tema: Tema;
  cambiaTema: (scelto: Tema) => void;
  movimentoUtente: MovimentoUtente;
  cambiaMovimentoUtente: (scelto: MovimentoUtente) => void;
  /** Di quanto è ingrandita l'interfaccia, o `null` finché non si sa. */
  zoom: number | null;
  /** Un gradino in su (`true`) o in giù. */
  cambiaZoom: (su: boolean) => void;
  /** La misura di serie. */
  zoomNormale: () => void;
  /** Il codice della lingua in uso. */
  lingua: string;
  cambiaLingua: (scelta: string) => void;
  scorciatoie: Associazioni;
  cambiaScorciatoie: (nuove: Associazioni) => void;
  /** La preferenza dell'accento che segue la copertina. */
  accentoDinamico: boolean;
  cambiaAccentoDinamico: (attivo: boolean) => Promise<void>;
  /**
   * Il tema e la lingua salvati si rileggono alla prossima `avvio`: un profilo
   * applicato li ha cambiati sotto i piedi della finestra. Vedi `dopoProfilo`
   * in `App`.
   */
  dimenticaTema: () => void;
  /** Rilegge la preferenza dell'accento, dopo un profilo applicato. */
  rileggiAccento: () => void;
  /** Rilegge «meno movimento», dopo un profilo applicato. */
  rileggiMovimento: () => void;
}

/**
 * L'aspetto della finestra, per tutta la sua vita.
 *
 * Una volta sola, in `App`: legge e scrive preferenze del database, e due
 * copie si contenderebbero il documento.
 */
export function useAspetto({
  avvio,
  skinAttiva,
  lettore,
  accentoDaRimettere,
  segnalaErrore,
  ricarica,
}: IngressiAspetto): Aspetto {
  /**
   * Il tema, che adesso arriva dal database.
   *
   * Si parte da «sistema» e non dal valore salvato perché quel valore non è
   * ancora arrivato: `avvio()` è una chiamata. Il fotogramma prima che
   * risponda segue il sistema operativo, che è il ripiego giusto — e la
   * finestra non si mostra finché skin e tema non sono sul documento (vedi
   * `pronto`), quindi nessuno lo vede.
   */
  const [tema, setTema] = useState<Tema>("sistema");
  /** Il tema è già stato deciso in questa sessione. Vedi l'effetto più sotto. */
  const temaDeciso = useRef(false);
  /**
   * Quanto movimento vuole chi guarda, sotto quello che la skin dichiara.
   *
   * Si parte da «sistema» come per il tema, e per la stessa ragione: il valore
   * vero è nel database e arriva con una chiamata. Il fotogramma prima che
   * risponda è quello in cui vale la sola `prefers-reduced-motion`, che è il
   * ripiego giusto — chi ha una condizione dichiarata al sistema operativo non
   * vede movimento nemmeno lì.
   */
  const [movimentoUtente, setMovimentoUtente] =
    useState<MovimentoUtente>("sistema");
  /**
   * Di quanto è ingrandita l'interfaccia, o `null` finché non si sa.
   *
   * `null` e non `1`, al contrario del movimento, e la differenza qui non è
   * accademica: la finestra nasce nascosta e a mostrarla è `pronto`. Se lo
   * zoom non fosse fra le cose che si aspettano, chi ha scelto 1,5 vedrebbe il
   * primo fotogramma alla misura di serie e il secondo alla sua — lo stesso
   * difetto del fotogramma scuro e di quello in italiano, con le misure.
   *
   * Il valore non si applica di qua: lo applica `zoom_avvio` alla finestra,
   * perché è lo zoom della WebView e non una scala nel foglio. Qui serve solo
   * a disegnarlo nelle impostazioni.
   */
  const [zoom, setZoom] = useState<number | null>(null);
  /**
   * La lingua in uso.
   *
   * Non è uno stato di questo hook: vive nel modulo `lingue`, perché i testi
   * servono anche fuori da React — `formato.ts` e `ipc.ts` ne chiamano le
   * funzioni senza essere componenti. Qui ci si **iscrive** e basta, e
   * l'iscrizione — che è di `App`, perché è `App` a chiamare questo hook — è
   * quel che ridisegna l'albero quando la lingua cambia.
   */
  const lingua = useLingua();
  /** La lingua è già stata decisa in questa sessione. Come il tema. */
  const linguaDecisa = useRef(false);
  /**
   * Le scorciatoie in uso.
   *
   * Derivate e non copiate in uno stato: la sorgente è `avvio.scorciatoie`, e
   * tenerne una seconda copia qui vorrebbe dire avere due verità che si
   * scostano appena una scrittura fallisce.
   */
  const scorciatoie = useMemo(
    () => leggiAssociazioni(avvio?.scorciatoie ?? null),
    [avvio?.scorciatoie],
  );

  // Il tema si riapplica quando cambia la scelta **o** quando cambia la skin:
  // una skin senza la variante del tema scelto resta nella sua base, e passare
  // da una con a una senza deve riscrivere `data-theme` invece di lasciarlo lì
  // a non fare niente.
  /**
   * Il tema che si sta **davvero** mostrando.
   *
   * Non `tema === "chiaro"`: una skin scura senza variante resta scura anche
   * con la preferenza sul chiaro, e una chiara senza variante resta chiara, e chi calcola qualcosa contro le superfici
   * del tema in uso — l'accento che segue la copertina — deve sapere quale
   * delle due ha vinto.
   */
  const [chiaro, setChiaro] = useState(false);

  useEffect(() => {
    setChiaro(applicaTema(tema, skinAttiva));
  }, [tema, skinAttiva]);

  /**
   * Il movimento chiesto da chi guarda, sul documento e nel database.
   *
   * Due effetti e non uno, come per il tema: qui si **applica** — cioè si
   * scrive l'attributo sulla radice a ogni cambio — e sotto si **legge** una
   * volta sola all'apertura. Tenerli insieme vorrebbe dire riscrivere il
   * documento anche quando non è cambiato niente.
   */
  useEffect(() => {
    applicaMovimento(movimentoUtente);
  }, [movimentoUtente]);

  /**
   * La preferenza, appena il database risponde.
   *
   * Non passa da `avvio`: è un comando suo, e la ragione sta scritta sopra
   * `movimento_ridotto` in `comandi.rs`. Una volta sola per apertura — questo
   * effetto non ha dipendenze — perché al contrario del tema non c'è nessuna
   * chiave vecchia da riconciliare: si legge, si applica, e da lì in poi
   * comanda il segmentato delle impostazioni.
   *
   * Un guasto qui non è un errore da mostrare: vorrebbe dire una finestra che
   * si apre con un avviso rosso per dire che il movimento è rimasto quello del
   * sistema, cioè per dire che non è successo niente.
   *
   * L'unica altra volta è un profilo applicato, che porta con sé la preferenza
   * (è fra quelle che viaggiano, vedi `profilo/mod.rs`): `dopoProfilo` in `App`
   * chiama `rileggiMovimento`, che è lo stesso corpo.
   */
  const rileggiMovimento = useCallback(() => {
    ipc
      .movimentoRidotto()
      .then((ridotto) => setMovimentoUtente(ridotto ? "ridotto" : "sistema"))
      .catch(() => undefined);
  }, []);
  useEffect(rileggiMovimento, [rileggiMovimento]);

  /**
   * Lo zoom, appena il database risponde — e applicato dall'altra parte.
   *
   * `zoomAvvio` legge **e** applica in un giro solo: lo zoom della WebView non
   * sopravvive alla chiusura, e il numero che torna di qua serve a disegnarlo
   * nelle impostazioni e a togliere l'attesa a `pronto`.
   *
   * Il `catch` scrive comunque il neutro invece di lasciare `null`, e non è
   * una formalità: senza, un database che non risponde lascerebbe la finestra
   * invisibile fino alla rete di sicurezza dei due secondi di `main.rs`, cioè
   * trasformerebbe una preferenza illeggibile in un'applicazione che non si
   * apre.
   */
  useEffect(() => {
    ipc
      .zoomAvvio()
      .then(setZoom)
      .catch(() => setZoom(1));
  }, []);

  /**
   * Un gradino in su o in giù.
   *
   * Al contrario del tema e del movimento, lo stato locale si muove **dopo**
   * la risposta e non prima: qui il valore che vale non lo decide questo
   * hook ma il nucleo, che ai due estremi della scala restituisce quello
   * di prima. Muoverlo prima vorrebbe dire mostrare per un fotogramma un
   * gradino che non esiste, ogni volta che si preme il tasto contro il fondo.
   * E non c'è niente da anticipare: quel che risponde al gesto è
   * l'ingrandimento, e a farlo è la finestra.
   */
  const cambiaZoom = useCallback(
    (su: boolean) => {
      ipc.zoomPasso(su).then(setZoom).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * La misura di serie: `Ctrl+0`, e il bottone delle impostazioni.
   *
   * Nessun argomento, e non è una scorciatoia di scrittura: attraverso l'IPC
   * dello zoom non passa **nessun numero**. Da questa parte si sanno i tre
   * gesti, e la scala — quali gradini esistono e in che ordine — sta soltanto
   * in `preferenze::SCALA_ZOOM`. Un secondo elenco di numeri qui sarebbe la
   * cosa che si scosta senza dirlo.
   */
  const zoomNormale = useCallback(() => {
    ipc.zoomNormale().then(setZoom).catch(segnalaErrore);
  }, [segnalaErrore]);

  /**
   * Cambia quanto movimento si vuole, e lo scrive.
   *
   * Lo stato locale si muove prima della scrittura, come per il tema: è un
   * cambio che si vede: le transizioni si fermano, ed è quella la risposta al
   * gesto.
   */
  const cambiaMovimentoUtente = useCallback(
    (scelto: MovimentoUtente) => {
      setMovimentoUtente(scelto);
      ipc
        .impostaMovimentoRidotto(scelto === "ridotto")
        .catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Da dove viene il tema: dal database, o dalla chiave vecchia.
   *
   * Una volta sola per apertura — il `ref` — perché `avvio` si rilegge dopo
   * ogni scansione e ogni cambio di cartella: senza la guardia, ogni ricarica
   * riscriverebbe il tema di chi non ne ha ancora scelto uno, e soprattutto
   * riporterebbe indietro quello appena scelto se la ricarica arrivasse fra la
   * scelta e la scrittura.
   */
  useEffect(() => {
    if (avvio === null || temaDeciso.current) return;
    temaDeciso.current = true;
    const salvato = avvio.tema;
    if (salvato === "scuro" || salvato === "chiaro" || salvato === "sistema") {
      setTema(salvato);
      // La chiave vecchia ha già fatto il suo mestiere: se restasse, un giorno
      // in cui il database non risponde tornerebbe a vincere lei.
      dimenticaRipiego();
      return;
    }
    const ripiego = temaDiRipiego();
    if (ripiego === null) return;
    setTema(ripiego);
    ipc.impostaTema(ripiego).then(dimenticaRipiego).catch(segnalaErrore);
  }, [avvio, segnalaErrore]);

  /**
   * Cambia il tema, e lo scrive.
   *
   * Qui lo stato locale si muove **prima** della scrittura, al contrario di
   * `cambiaAccentoDinamico`: il tema è un cambio che si vede: la finestra si
   * ridipinge, ed è quella la risposta al gesto. Aspettare il database
   * vorrebbe dire un ritardo visibile su ogni pressione per proteggersi da un
   * guasto che, se capita, si vede lo stesso — l'errore compare, e la prossima
   * apertura mostra la scelta di prima.
   */
  const cambiaTema = useCallback(
    (scelto: Tema) => {
      setTema(scelto);
      ipc.impostaTema(scelto).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Quale lingua parlare, appena il database risponde.
   *
   * Una volta sola per apertura — il `ref` — per la stessa ragione del tema:
   * `avvio` si rilegge dopo ogni scansione, e senza la guardia una ricarica
   * arrivata subito dopo un cambio di lingua rimetterebbe quella di prima.
   *
   * `scegli` fa il resto: la lingua salvata se il suo file c'è ancora, altrimenti
   * quella del sistema, altrimenti l'inglese. Il caso «c'era e non c'è più» non è
   * teorico — basta un profilo importato da un'installazione con più lingue — e
   * il ripiego lì è meglio di un'interfaccia di sole chiavi.
   */
  useEffect(() => {
    if (avvio === null || linguaDecisa.current) return;
    linguaDecisa.current = true;
    applicaLingua(scegli(avvio.lingua, navigator.language));
  }, [avvio]);

  /**
   * Le voci del menù dell'icona nell'area di notifica.
   *
   * Scendono da qui perché i testi che si leggono stanno in `lingue/`, dove
   * `strumenti/lingue.js` controlla che ogni lingua le abbia tutte. Scritte in
   * Rust sarebbero le uniche due fuori da quel controllo, e il sintomo — un
   * menù metà in una lingua e metà nell'altra — somiglia troppo a una svista di
   * traduzione perché qualcuno lo segnali.
   *
   * Sull'iscrizione alla lingua e non su `avvio`: parte a ogni cambio, che è
   * esattamente quando il menù andrebbe altrimenti alla deriva. È anche il
   * momento in cui l'icona compare la prima volta — senza etichette non c'è un
   * menù da costruire, e `vassoio.rs` aspetta questa chiamata.
   */
  useEffect(() => {
    ipc
      .vassoioLingua(
        t("tray.show"),
        t("tray.quit"),
        t("tray.play"),
        t("tray.pause"),
        t("tray.next"),
        t("tray.previous"),
      )
      .catch(() => {
        /* Un menù che non si rietichetta non è una cosa da annunciare: chi non
           ha acceso il secondo piano non ha nemmeno un'icona, e chi l'ha acceso
           la ritrova nella lingua di prima. Segnalarlo sarebbe un errore rosso
           per una cosa che nessuno stava guardando. */
      });
  }, [lingua]);

  /**
   * Cambia la lingua, e la scrive.
   *
   * Come il tema: il documento si muove **prima** della scrittura, perché il
   * cambio si vede ed è quella la risposta al gesto. Se il database non prende
   * la modifica, l'errore compare e la prossima apertura mostra la lingua di
   * prima — cosa che si nota subito, al contrario di un'attesa su ogni click.
   */
  const cambiaLingua = useCallback(
    (scelta: string) => {
      applicaLingua(scelta);
      ipc.impostaLingua(scelta).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Riassegna le scorciatoie.
   *
   * Si scrive e si rilegge: `scorciatoie` è derivata da `avvio`, quindi
   * l'elenco a schermo si muove quando il database ha davvero preso la
   * modifica. Una scorciatoia che compare nella scheda e non funziona sarebbe
   * la peggiore delle due bugie possibili qui.
   */
  const cambiaScorciatoie = useCallback(
    (nuove: Associazioni) => {
      ipc
        .impostaScorciatoie(scriviAssociazioni(nuove))
        .then(ricarica)
        .catch(segnalaErrore);
    },
    [ricarica, segnalaErrore],
  );

  useEffect(
    () =>
      seguiIlSistema(
        () => ({ tema, skin: skinAttiva }),
        setChiaro,
      ),
    [tema, skinAttiva],
  );

  /**
   * L'accento segue la copertina del disco che sta suonando.
   *
   * Tre righe qui e nessuna decisione: il colore lo estrae `aether-app` dalla
   * miniatura, e se sia leggibile lo decide `aether-skin` in OKLCH, dove vive
   * `contrast_ratio`. La finestra scrive quel che le viene detto — è la stessa
   * regola per cui il foglio della skin arriva già compilato.
   *
   * `null` è la risposta normale, non un guasto: un disco senza copertina, una
   * copertina in bianco e nero, una skin che dichiara di non volerlo, o una
   * tonalità che a nessuna chiarezza regge 4,5:1 contro le superfici. In tutti
   * e quattro i casi vince l'accento che la skin ha scritto.
   */
  const [accentoDinamico, setAccentoDinamico] = useState(false);
  useEffect(() => {
    ipc.accentoDinamico().then(setAccentoDinamico).catch(segnalaErrore);
  }, [segnalaErrore]);

  /**
   * Accende o spegne la preferenza.
   *
   * Lo stato locale si muove **dopo** la scrittura e con quel che il nucleo
   * riporta, non prima: un interruttore che scatta e poi torna indietro perché
   * il database non ha risposto è peggio di uno che scatta un attimo dopo. Il
   * ricalcolo dell'accento lo fa l'effetto qui sotto, che ha `accentoDinamico`
   * fra le dipendenze — spegnerlo toglie le variabili, accenderlo le rimette.
   */
  const cambiaAccentoDinamico = useCallback(
    async (attivo: boolean) => {
      try {
        setAccentoDinamico(await ipc.accentoDinamicoAttiva(attivo));
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [segnalaErrore],
  );

  useEffect(() => {
    // Il brano può cambiare mentre la risposta è in volo — succede saltando
    // avanti in fretta — e la risposta di prima dipingerebbe la finestra col
    // colore del disco sbagliato. Due guardie: `attuale` per le dipendenze
    // qui sotto, che rifanno l'effetto; `giro` per la copertina, che cambia
    // dentro di lui.
    let attuale = true;
    let giro = 0;
    let vista: string | null | undefined = undefined;
    const segui = () => {
      const copertina = lettore.leggi().brano?.coverArtHash ?? null;
      // Tutto quel che dell'archivio non è la copertina — il volume, la
      // pausa, un brano nuovo dello stesso disco — passa senza chiedere niente.
      if (copertina === vista) return;
      vista = copertina;
      const mio = (giro += 1);
      ipc
        .accentoCopertina(copertina, chiaro)
        .then((variabili) => {
          if (attuale && mio === giro) applicaAccento(variabili);
        })
        // Un accento che non si è potuto calcolare non è un guasto da mostrare:
        // la fascia rossa sopra la pagina per dire che il colore dei pulsanti è
        // rimasto quello di prima — cioè, a guardarla, che non è successo niente.
        // Si tiene l'accento che c'è, quello del disco prima o quello della skin,
        // e il prossimo brano riprova da sé.
        .catch(() => undefined);
    };
    segui();
    const smetti = lettore.iscrivi(segui);
    return () => {
      attuale = false;
      smetti();
    };
  }, [lettore, chiaro, skinAttiva, accentoDinamico, accentoDaRimettere]);

  /**
   * Mostra la finestra, una volta sola, quando c'è qualcosa di giusto da
   * vedere.
   *
   * La finestra nasce nascosta. Fino a qui `backgroundColor` di
   * `tauri.conf.json` era il fondo di `plain`, dipinto dal sistema operativo
   * **prima** che esistesse una pagina: chi sceglieva una skin chiara vedeva un
   * fotogramma scuro a ogni avvio, ed era il primo dei difetti noti.
   *
   * Due fotogrammi e non uno: il primo lascia che skin e tema arrivino al
   * documento, il secondo è quello in cui il motore li ha davvero disegnati.
   * Mostrarla nel mezzo rimetterebbe il difetto un fotogramma più in là.
   *
   * Si aspetta anche `avvio`, e non solo la skin, da quando l'interfaccia ha una
   * lingua: la lingua salvata arriva di lì, e mostrare la finestra prima
   * vorrebbe dire far leggere a chi ha scelto l'inglese un fotogramma di
   * italiano — lo stesso difetto del colore, con le parole.
   *
   * E si aspetta anche lo zoom, per la terza volta la stessa ragione: chi ha
   * scelto 1,5 non deve vedere un fotogramma alla misura di serie. Lo zoom
   * arriva sempre, riuscito o no — vedi il suo effetto — quindi questa
   * attesa non può diventare una finestra che non si apre.
   */
  const mostrata = useRef(false);
  useEffect(() => {
    if (
      mostrata.current ||
      skinAttiva === null ||
      avvio === null ||
      zoom === null
    )
      return;
    mostrata.current = true;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        ipc.pronto().catch(segnalaErrore);
      }),
    );
  }, [skinAttiva, avvio, zoom, segnalaErrore]);

  /*
   * Un profilo è appena stato applicato. `temaDeciso` torna falso apposta — è
   * l'unico caso in cui il tema salvato cambia sotto i piedi della finestra, e
   * la guardia che impedisce di rileggerlo a ogni ricarica qui va tolta di
   * mano. La lingua ha la stessa guardia e viaggia nello stesso profilo, quindi
   * torna da decidere anche lei: senza, il profilo diceva «inglese» e la
   * finestra restava in italiano fino al riavvio. Il resto del giro lo fa
   * `dopoProfilo` in `App`.
   */
  const dimenticaTema = useCallback(() => {
    temaDeciso.current = false;
    linguaDecisa.current = false;
  }, []);
  const rileggiAccento = useCallback(() => {
    ipc.accentoDinamico().then(setAccentoDinamico).catch(segnalaErrore);
  }, [segnalaErrore]);

  return useMemo(
    () => ({
      tema,
      cambiaTema,
      movimentoUtente,
      cambiaMovimentoUtente,
      zoom,
      cambiaZoom,
      zoomNormale,
      lingua,
      cambiaLingua,
      scorciatoie,
      cambiaScorciatoie,
      accentoDinamico,
      cambiaAccentoDinamico,
      dimenticaTema,
      rileggiAccento,
      rileggiMovimento,
    }),
    [
      tema,
      cambiaTema,
      movimentoUtente,
      cambiaMovimentoUtente,
      zoom,
      cambiaZoom,
      zoomNormale,
      lingua,
      cambiaLingua,
      scorciatoie,
      cambiaScorciatoie,
      accentoDinamico,
      cambiaAccentoDinamico,
      dimenticaTema,
      rileggiAccento,
      rileggiMovimento,
    ],
  );
}
