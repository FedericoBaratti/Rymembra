/**
 * Parte del contratto con il nucleo: il collegamento via internet fra i
 * dispositivi di una persona, e «Sfoglia <dispositivo>». La riesporta `ipc.ts`:
 * si importa da lì.
 *
 * Come in `servizi.ts`, nessun campo è facoltativo: un
 * `#[serde(rename_all = "camelCase")]` dimenticato di là deve diventare un
 * errore di `tsc`, non un `undefined` in silenzio.
 */

/** «Solo Wi-Fi»: che fare dei brani degli altri sui dati mobili. */
export type DatiMobili = "chiedi" | "si" | "no";

/** Un dispositivo collegato (`lontano/ipc.rs`, `DispositivoIpc`). */
export interface DispositivoLontano {
  /** La sua chiave pubblica: il suo nome nei comandi. */
  chiave: string;
  nome: string;
  /** `windows`, `android`. */
  piattaforma: string;
  /** L'id che ha nella sincronia, per unire le voci della pagina. */
  dispositivo: string;
  /** Risponde adesso. */
  online: boolean;
  /** L'ultima volta che ha risposto. */
  vistoMs: number | null;
  /** Quando ci si è collegati. */
  associatoMs: number;
  /** Dall'altra parte l'hanno dimenticato. */
  revocato: boolean;
  /** La versione di Rymembra che dice di avere, quando risponde. */
  versione: string | null;
  insiemeAcceso: boolean;
  insiemeOspita: boolean;
  /** La riga fra i PC fidati di «Ascolta insieme» con la sua chiave, se c'è. */
  insieme: number | null;
  /** Da dove passa adesso la connessione; `null` senza una connessione viva. */
  via: "diretto" | "relay" | null;
}

/** Il QR da mostrare: il link, e la matrice. */
export interface InvitoLontano {
  /** `rymembra://collega/…`, da copiare. */
  link: string;
  /** Moduli per lato. */
  lato: number;
  /** I moduli riga per riga, `1` scuro e `0` chiaro. */
  moduli: string;
  /** Quando non vale più. */
  scadeMs: number;
}

/** Qualcuno ha letto il QR e aspetta un sì o un no. */
export interface RichiestaLontano {
  id: number;
  /** Il nome che **dichiara**: lo sceglie lui. */
  nome: string;
  piattaforma: string;
  /** L'impronta della sua chiave: lui la mostra mentre aspetta. */
  impronta: string;
}

/**
 * Un link di collegamento esaminato prima di collegarsi (`lontanoLinkEsamina`):
 * con chi ci si collegherebbe.
 */
export interface LinkLontano {
  /** Da ripetere a `lontanoCollega`. */
  link: string;
  /** Il nome che chi mostra il QR dichiara; `null` se il link non ne porta. */
  nome: string | null;
  /** L'impronta della sua chiave: la stessa che lui mostra accanto al QR. */
  impronta: string;
}

/** Il pannello del collegamento (`StatoLontanoIpc`). */
export interface StatoLontano {
  /** «Raggiungibile da internet». */
  attivo: boolean;
  /** Il nodo è acceso adesso: sul telefono può essere spento con l'interruttore acceso. */
  acceso: boolean;
  /** La chiave di questo dispositivo, a nodo acceso. */
  chiave: string | null;
  /**
   * La sua impronta, a nodo acceso: accanto al QR, e mentre si aspetta una
   * conferma, perché sull'altro schermo la si confronti.
   */
  impronta: string | null;
  /** Il relay proprio, se c'è. */
  relay: string | null;
  datiMobili: DatiMobili;
  /** Perché il nodo non si accende: la chiave del testo. */
  errore: string | null;
  dispositivi: DispositivoLontano[];
  invito: InvitoLontano | null;
  richieste: RichiestaLontano[];
}

/** Il pezzo di un'immagine CD, in fotogrammi da 1/75 di secondo. */
export interface SegmentoRemoto {
  inizioCd: number;
  fineCd: number | null;
}

/**
 * Un brano di un altro dispositivo (`rymembra_app::library::condivisa::BranoRemoto`).
 *
 * La finestra lo rimanda intero a `lontanoSuona` e compagni: il nucleo non
 * tiene gli elenchi, registra quel che gli arriva.
 */
export interface BranoRemoto {
  /** L'id nella libreria **dell'altro**. */
  id: number;
  title: string;
  artist: string;
  album: string;
  /** L'impronta dell'album, da ripetere a `lontanoBraniAlbum`. */
  albumKey: string | null;
  trackNumber: number | null;
  discNumber: number | null;
  durationMs: number;
  year: number | null;
  /** Già nella forma del protocollo delle copertine (`lontano-…`). */
  coverArtHash: string | null;
  /** Quante volte l'ha ascoltato il proprietario: solo da leggere. */
  playCount: number;
  liked: boolean;
  rating: number;
  estensione: string;
  dimensione: number;
  versione: string;
  segmento: SegmentoRemoto | null;
}

/** L'ordine dei brani dell'altro: gli stessi nomi di `ipc.brani`. */
export type OrdineRemoto = "recenti" | "ascoltati" | "titolo" | "scaffale";

/** A che punto è «Scarica qui» (`lontano/scarico.rs`, `StatoScaricoIpc`). */
export interface ScaricoLontano {
  /** Il brano che sta scendendo; `null` a coda ferma. */
  titolo: string | null;
  byte: number;
  totale: number;
  /** I brani che aspettano dopo quello in corso. */
  inCoda: number;
  /** Da quando la coda è ripartita. */
  fatti: number;
  /** C'erano già, o sono brani di un'immagine CD. */
  saltati: number;
  falliti: number;
  /** Il codice dell'ultimo guasto. */
  errore: string | null;
}

/** In che punto è la lettura del catalogo di un dispositivo. */
export type FaseSpecchio = "inattivo" | "leggo" | "fine" | "errore" | "nonSupportato";

/**
 * Quel che la finestra sa del catalogo di un dispositivo collegato
 * (`lontano/specchio.rs`, `AvanzamentoSpecchio`). Arriva con l'evento
 * `lontano:specchio` a ogni passo, e `lontanoSpecchioStato` dà l'ultimo di
 * ciascuno a chi arriva dopo.
 */
export interface AvanzamentoSpecchio {
  /** La chiave del dispositivo. */
  dispositivo: string;
  fase: FaseSpecchio;
  /** Righe lette in questo giro, su `totali`. */
  fatti: number;
  totali: number;
  /** Righe di questo dispositivo in libreria, a giro finito. */
  righe: number;
  /** Righe che il dispositivo non manda più, in attesa di una conferma per sparire. */
  inAttesa: number;
  /** Quando è finito l'ultimo giro riuscito, in millisecondi. */
  ultimoMs: number | null;
  /** Il codice dell'errore, se il giro non è riuscito. */
  errore: string | null;
}

/** Cosa succederebbe alle playlist dimenticando un dispositivo. */
export interface AnteprimaDimentica {
  righe: number;
  voci: number;
  rimappabili: number;
  perse: number;
  /** In quante playlist si perde qualcosa: il numero da dire a chi conferma. */
  playlistChePerdono: number;
}

/** L'esito dello scanner del QR (solo telefono). */
export interface Scansione {
  /** Il link letto; `null` se si è annullato. */
  link: string | null;
  /** La fotocamera non è stata concessa. */
  negato: boolean;
}
