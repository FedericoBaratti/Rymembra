/**
 * Le voci dei menù contestuali della libreria: su dei brani, su un album, su
 * una playlist, su un artista.
 *
 * # Cosa sta qui, e cosa no
 *
 * **Quali** voci, in che ordine e a quali condizioni: è la parte che si
 * sbagliava senza che nessuno lo vedesse — una voce «Elimina dal disco» su un
 * brano che un file non ce l'ha, «Mostra nella cartella» su un brano di
 * catalogo — e qui si prova da sola (`test/menuBrani.test.ts`). **Cosa fa**
 * ogni voce resta a chi apre il menù (`App`), che ha in mano la coda, le
 * finestrelle e le pagine: arriva come un oggetto di azioni, e i testi come
 * una funzione `t`. Così questo modulo non tocca né il nucleo né le lingue, e
 * `node --test` lo carica senza niente intorno.
 */
import { eNuvola, nomeServizioDi } from "./cartelleNuvola";
import { idDiLibreria } from "./insieme";
import type { Brano, Playlist } from "./ipc";
import type { Chiave, Valori } from "./lingue";
import type { Voce } from "./Menu";

/**
 * Una chiave tradotta: `t` di `lingue`, o una finta nelle prove.
 *
 * I valori servono a una voce sola, «Apri su OneDrive»: il nome del servizio
 * è un nome proprio e non si traduce, ma la frase intorno sì.
 */
export type Traduci = (chiave: Chiave, valori?: Valori) => string;

/** Quel che si può fare con dei brani dal menù. */
export interface AzioniBrani {
  radio: (seme: number) => void;
  dopo: (brani: number[]) => void;
  accoda: (brani: number[]) => void;
  aggiungiAPlaylist: (brani: number[]) => void;
  vaiAlDisco: (brano: number) => void;
  vaiAllArtista: (brano: number) => void;
  mostraNellaCartella: (brano: number) => void;
  apriPagina: (brano: number) => void;
  /** La pagina del file sul suo servizio: «Mostra nella cartella» di un brano del cloud. */
  apriSulWeb: (brano: number) => void;
  togli: (brani: number[]) => void;
  elimina: (brani: number[]) => void;
}

/**
 * Le voci del menù su una selezione di brani.
 *
 * `inMano` è l'elenco che si sta guardando, letto al momento del tasto
 * destro: dice di un brano se è un file o un brano di catalogo. Un brano che
 * non vi si trova — il menù aperto da un pannello che mostra un'altra lista —
 * lascia le voci come stavano prima di distinguere i due casi, e il nucleo si
 * difende comunque da sé.
 */
export function vociBrani(
  scelti: number[],
  inMano: (id: number) => Brano | undefined,
  fai: AzioniBrani,
  t: Traduci,
  /**
   * Questo PC è in una sessione di «Ascolta insieme»: la radio non c'è.
   *
   * Il nucleo la rifiuta per tutti e due i ruoli (`insieme/strada.rs`), e una
   * voce che risponde con un errore rosso a un clic ragionevole è peggio di
   * una voce che non c'è. Un valore e non una domanda al lettore: chi apre il
   * menù lo legge al momento del gesto, e questo modulo resta senza nucleo.
   */
  inSessione = false,
  /**
   * Chi ospita la sessione non ha dato la coda a questo PC: «Dopo» e «In
   * coda» non ci sono, per la stessa ragione della radio qui sopra. Vedi
   * `gestoPermesso`.
   */
  senzaCoda = false,
): Voce[] {
  /*
   * Solo le righe della libreria di questo PC.
   *
   * Una voce di «Ascolta insieme» (id negativo) è un brano che sta in coda
   * perché la sessione ce l'ha messo: preferito, playlist, disco, artista,
   * cartella e cancellazione scriverebbero tutti su una riga che qui non c'è.
   * Oggi gli elenchi da cui si apre questo menù sono tutti della libreria e
   * una voce non ci arriva; il filtro sta qui perché è qui che si decide quali
   * voci esistono, e il primo elenco che mostrerà la coda di sessione non
   * dovrà ricordarsene. Senza nessuna riga di libreria, nessuna voce: un menù
   * di gesti che non vanno da nessuna parte è peggio di nessun menù.
   */
  const brani = scelti.filter(idDiLibreria);
  if (brani.length === 0) return [];
  // La radio ha **un** seme, e con più brani selezionati non si sa quale:
  // la voce sparisce invece di far finta di scegliere. È lo stesso motivo
  // per cui «rinomina» non compare su una selezione multipla.
  const primo = brani.length === 1 ? brani[0] : undefined;
  const conRadio = primo !== undefined && !inSessione;
  /*
   * Il brano solo sta nel cloud: allora «Mostra nella cartella» non c'è — sul
   * disco una cartella non ce l'ha — e al suo posto c'è la pagina del file sul
   * servizio, che è la risposta alla stessa domanda («dov'è questo?»).
   */
  const delPrimo = primo === undefined ? undefined : inMano(primo);
  const servizioDelPrimo = eNuvola(delPrimo?.path) ? nomeServizioDi(delPrimo?.path) : null;
  /*
   * Le tre voci che su un brano solo mancavano: portarsi al suo disco, al
   * suo artista, e al suo file. Il menù aveva «radio», «dopo», «in coda» e
   * «a una playlist» — cioè tutto quel che si fa **con** il brano e niente
   * di quel che si fa **da** il brano, che è la domanda più comune davanti a
   * una riga di un elenco lungo: «di che disco è questo?».
   */
  const daUnBrano: Voce[] =
    primo === undefined
      ? []
      : [
          { etichetta: t("menu.goToAlbum"), azione: () => fai.vaiAlDisco(primo) },
          { etichetta: t("menu.goToArtist"), azione: () => fai.vaiAllArtista(primo) },
          // «Mostra nella cartella» solo su un file. Un brano di catalogo
          // in nessuna cartella sta, e la voce che lo promettesse
          // risponderebbe con un errore a un clic perfettamente
          // ragionevole. Il comando si difende comunque da sé — un comando
          // è una porta — ma una porta che non si apre non va messa in
          // vista.
          // Il brano non trovato nell'elenco — il menù aperto da un
          // pannello che mostra un'altra lista — lascia la voce dov'è:
          // è quel che si faceva prima di distinguere i due casi, e il
          // comando si difende comunque da sé.
          ...(servizioDelPrimo !== null
            ? [
                {
                  etichetta: t("menu.openOnService", { servizio: servizioDelPrimo }),
                  azione: () => fai.apriSulWeb(primo),
                },
              ]
            : delPrimo?.fonte == null
              ? [
                  {
                    etichetta: t("menu.showInFolder"),
                    azione: () => fai.mostraNellaCartella(primo),
                  },
                ]
              : []),
          // E il contrario: la pagina pubblica esiste solo per un brano che
          // viene da un catalogo. È il gesto che «Mostra nella cartella»
          // è per un file — «fammi vedere da dove viene» — e per certe
          // licenze non è una curiosità ma una condizione d'uso, per cui
          // qui c'è e in «In riproduzione» c'è scritta per esteso.
          ...(inMano(primo)?.fontePagina != null
            ? [{ etichetta: t("track.openPage"), azione: () => fai.apriPagina(primo) }]
            : []),
        ];
  /*
   * «Elimina dal disco» solo se sul disco c'è qualcosa da eliminare.
   *
   * Su un brano in streaming la voce c'era, e la conferma prometteva che i
   * file sarebbero finiti nel Cestino e tornati con «Scansiona»: tre
   * affermazioni false di fila per un brano che un file non ce l'ha. Se
   * tutti i brani scelti vengono da un catalogo la voce non si mette; se
   * uno solo è un file, o non si trova nell'elenco — il menù aperto da un
   * pannello con un'altra lista — resta, e il nucleo tratta i brani di
   * catalogo come «togli dalla libreria», che è quel che per loro vuol dire.
   */
  // Una riga per volta, e solo finché servono: con «Seleziona tutto» su
  // una lista rada gli scelti sono decine di migliaia, quasi tutti non in
  // mano — e il primo che non si trova chiude la domanda.
  const soloStreaming =
    brani.length > 0 &&
    brani.every((id) => {
      const brano = inMano(id);
      return brano !== undefined && brano.fonte != null;
    });
  /*
   * E se stanno tutti nel cloud la voce c'è, ma spenta, con la sua ragione:
   * Rymembra su quei servizi ha solo il permesso di leggere. Spenta e non
   * tolta, al contrario dello streaming: un brano del cloud è un file vero, e
   * chi cerca «Elimina dal disco» deve sapere perché qui non si può e dove si
   * fa. Con un brano solo del disco in mezzo resta accesa, con la regola di
   * sopra: è il nucleo che tratta quelli del cloud (`cloud.readOnly`).
   */
  const soloNuvola =
    brani.length > 0 &&
    brani.every((id) => {
      const brano = inMano(id);
      return brano !== undefined && eNuvola(brano.path);
    });
  return [
    ...(conRadio ? [{ etichetta: t("action.radio"), azione: () => fai.radio(primo) }] : []),
    ...(senzaCoda
      ? []
      : [
          { etichetta: t("action.playNext"), azione: () => fai.dopo(brani) },
          { etichetta: t("action.enqueue"), azione: () => fai.accoda(brani) },
        ]),
    { etichetta: t("action.addToPlaylist"), azione: () => fai.aggiungiAPlaylist(brani) },
    ...daUnBrano,
    /*
     * Le due distruttive, in fondo e in quest'ordine.
     *
     * In fondo perché sono le uniche voci del menù che tolgono qualcosa, e
     * la distanza dal puntatore è l'unica protezione che un menù sa dare
     * contro il clic sbagliato. In quest'ordine perché la seconda contiene
     * la prima: chi elimina dal disco toglie anche dalla libreria, e
     * l'ordine inverso metterebbe la più grave sotto il dito per prima.
     *
     * Tutte e due aprono una conferma — i puntini lo dicono — e non perché
     * una domanda renda prudenti: un voto, un preferito e un conteggio
     * d'ascolto costruiti in anni se ne vanno con la riga, e di quello non
     * c'è annulla da offrire. Dove un annulla c'è, come per la coda
     * sostituita, si offre quello e non si chiede niente.
     */
    { etichetta: t("menu.removeFromLibrary"), azione: () => fai.togli(brani) },
    ...(soloStreaming
      ? []
      : soloNuvola
        ? [
            {
              etichetta: t("menu.deleteFromDisk"),
              azione: () => undefined,
              spenta: t("errors.cloud.readOnly"),
            },
          ]
        : [{ etichetta: t("menu.deleteFromDisk"), azione: () => fai.elimina(brani) }]),
  ];
}

/** Quel che si può fare con un album dal menù: i brani si chiedono al momento. */
export interface AzioniAlbum {
  radio: () => void;
  dopo: () => void;
  accoda: () => void;
}

/**
 * Le voci del menù di una scheda album.
 *
 * `inSessione` toglie la radio e `senzaCoda` «Dopo» e «In coda», come in
 * [`vociBrani`]. Senza nessuna delle tre il menù è vuoto, e un menù vuoto non
 * si apre.
 */
export function vociAlbum(
  fai: AzioniAlbum,
  t: Traduci,
  inSessione = false,
  senzaCoda = false,
): Voce[] {
  return [
    // Il seme è la **prima traccia** e non un brano a caso del disco: è
    // quella che chi apre un album sente per prima, quindi è quella di
    // cui sta chiedendo «portami dove porta questo».
    ...(inSessione ? [] : [{ etichetta: t("action.radio"), azione: fai.radio }]),
    ...(senzaCoda
      ? []
      : [
          { etichetta: t("action.playNext"), azione: fai.dopo },
          { etichetta: t("action.enqueue"), azione: fai.accoda },
        ]),
  ];
}

/** Le prime tre voci del menù di una riga del pannello Cartelle. */
export interface AzioniCartella {
  riproduci: () => void;
  dopo: () => void;
  accoda: () => void;
}

/**
 * Le voci di coda del menù di una riga del pannello Cartelle: «Riproduci»,
 * «Dopo», «In coda». Il resto del menù (playlist, «Mostra nella cartella») lo
 * aggiunge il pannello, che sa dove sta la riga.
 *
 * `senzaCoda` toglie «Dopo» e «In coda», come in [`vociBrani`]: in una
 * sessione dove chi ospita non ha dato la coda a questo PC il pannello le
 * offriva lo stesso, e premerle portava la fascia rossa del rifiuto del
 * nucleo.
 */
export function vociCartella(fai: AzioniCartella, t: Traduci, senzaCoda = false): Voce[] {
  return [
    { etichetta: t("action.play"), azione: fai.riproduci },
    ...(senzaCoda
      ? []
      : [
          { etichetta: t("action.playNext"), azione: fai.dopo },
          { etichetta: t("action.enqueue"), azione: fai.accoda },
        ]),
  ];
}

/** Quel che si può fare con una playlist dal menù della navigazione. */
export interface AzioniPlaylist {
  suona: () => void;
  rinomina: () => void;
  regole: () => void;
  cancella: () => void;
  esporta: () => void;
}

/** Le voci del menù di una playlist nella barra di navigazione. */
export function vociPlaylist(p: Playlist, fai: AzioniPlaylist, t: Traduci): Voce[] {
  return [
    { etichetta: t("action.play"), azione: fai.suona },
    // Una playlist automatica non si rinomina: la chiave è il nome, e
    // cambiarlo ne farebbe una nuova che le regole del vecchio database non
    // conoscono più. Si cambiano le sue regole.
    p.isSmart
      ? { etichetta: t("menu.editRules"), azione: fai.regole }
      : { etichetta: t("menu.rename"), azione: fai.rinomina },
    // Senza domanda, e con un «Annulla» dopo: la cancellazione è già
    // scritta quando l'avviso compare, e il nucleo tiene quel che serve a
    // rimetterla com'era — vedi `playlists::Cancellata`. Una domanda a ogni
    // eliminazione insegnerebbe a rispondere «sì» senza leggere.
    { etichetta: t("common.delete"), azione: fai.cancella },
    // L'esportazione vale per tutte e due: una playlist intelligente si
    // esporta con i brani che ha **adesso**, che è quel che un M3U può
    // rappresentare — le regole non attraversano il formato, e portarle via
    // fingendo di sì sarebbe peggio che non esportarla.
    { etichetta: t("menu.exportFile"), azione: fai.esporta },
  ];
}

/** Le voci del menù di una scheda artista. */
export function vociArtista(
  fai: { apri: () => void; cercaPerNome: () => void },
  t: Traduci,
): Voce[] {
  return [
    { etichetta: t("menu.open"), azione: fai.apri },
    { etichetta: t("menu.searchName"), azione: fai.cercaPerNome },
  ];
}
