/**
 * Il collegamento via internet nella finestra: lo stato del pannello, il QR da
 * disegnare, e la domanda «usare i dati mobili?».
 *
 * # Un archivio, come «Ascolta insieme»
 *
 * Il pannello lo decide il nucleo e arriva intero con `lontano:stato` a ogni
 * cambiamento. Lo leggono la scheda delle Impostazioni, la barra laterale (i
 * dispositivi online), la pagina Altro del telefono e la pagina «Sfoglia»:
 * sta in un archivio (`archivio.ts`), e l'ascolto lo apre un componente solo,
 * `LontanoInApp`, montato una volta in `App`.
 *
 * # Il QR
 *
 * Il nucleo manda la matrice dei moduli, non un'immagine: la finestra la
 * disegna come un `<svg>` con un percorso solo (`percorsoQr`). Niente HTML che
 * arriva da fuori, e la CSP resta com'è.
 *
 * Senza React a runtime oltre agli hook in fondo: le prove di `test/` lo
 * caricano con `node --test`.
 */
import { creaArchivio, sceltaRicordata, ugualiInSuperficie } from "./archivio";
import { ordinaFidati } from "./insieme";
import { eventiEFotografia } from "./ultima";
import type { Chiave, Valori } from "./lingue";
import type {
  Album,
  BranoRemoto,
  DispositivoLontano,
  DispositivoSincronia,
  ErroreIpc,
  FidatoInsieme,
  LinkLontano,
  StatoLontano,
  ScaricoLontano,
} from "./ipc";

/** Il pannello, `null` finché il nucleo non ha risposto. */
export const archivioLontano = creaArchivio<StatoLontano | null>(null);

/**
 * Gli eventi `lontano:stato` contro le risposte dei comandi (`ultima.ts`).
 *
 * L'archivio è uno solo per tutta la finestra, e il contatore sta accanto a
 * lui, a livello di modulo: lo scrivono l'ascolto di `LontanoInApp`, la scheda
 * dei dispositivi e `LontanoInApp` stesso, e nessuno dei tre deve ricoprire un
 * evento più nuovo con una risposta chiesta prima di lui (R13-10).
 */
const eventiDelPannello = eventiEFotografia();

/**
 * Un `lontano:stato` mandato dal nucleo: il pannello com'è adesso. Si scrive e
 * si conta, e le risposte dei comandi partiti prima non lo ricoprono più.
 */
export function eventoDelPannello(stato: StatoLontano): void {
  eventiDelPannello.evento();
  archivioLontano.scrivi(stato);
}

/**
 * Chiamala **prima** di mandare un comando che risponde col pannello: restituisce
 * chi adotta la risposta, che si scrive solo se nel frattempo non è arrivato un
 * evento. Per chi deve fare altro fra il comando e la scrittura (`fai`, in
 * `parti/Dispositivi.tsx`); gli altri usano [`comandoDelPannello`].
 *
 * Perché scartarla è giusto: il nucleo pubblica l'evento a ogni cambio del
 * pannello (`lontano/filo.rs`, `pubblica`, solo se il testo è cambiato), e un
 * comando che lo cambia ha il suo evento. Quello arrivato in mezzo è dunque
 * fresco quanto la risposta o più; la risposta è il passato di un istante fa, e
 * posata sopra di lui lo rimetterebbe a schermo fino al cambio dopo — che può
 * essere a un minuto (`SCRIVI_VISTO_OGNI`).
 */
export function chiediIlPannello(): (risposta: StatoLontano) => void {
  const vale = eventiDelPannello.chiedi();
  return (risposta) => {
    if (vale()) archivioLontano.scrivi(risposta);
  };
}

/**
 * Manda un comando che risponde col pannello, e ne adotta la risposta se non è
 * arrivato un evento nel frattempo. La risposta si restituisce sempre: chi ha
 * chiesto la usa per altro (un collegamento che parte, un invito).
 */
export async function comandoDelPannello(
  comando: () => Promise<StatoLontano>,
): Promise<StatoLontano> {
  const adotta = chiediIlPannello();
  const risposta = await comando();
  adotta(risposta);
  return risposta;
}

/**
 * Il link di collegamento che aspetta il sì della persona, con chi ci si
 * collegherebbe (`lontanoLinkEsamina`): aperto da fuori, letto dallo scanner o
 * incollato, passa sempre dalla stessa finestrella, in `LontanoInApp`.
 */
export const archivioLink = creaArchivio<LinkLontano | null>(null);

/**
 * Collegarsi, o mostrare un QR, chiede prima un sì per accendere
 * «Raggiungibile da internet»: il nucleo non lo accende da sé
 * (`lontano.spento`). Con l'interruttore acceso basta: sul telefono il nodo
 * addormentato lo sveglia il comando.
 */
export function serveAccendere(stato: StatoLontano | null): boolean {
  return stato?.attivo !== true;
}

/**
 * Un gesto su un brano dell'altro che sui dati mobili aspetta un sì: la
 * finestrella «Usare i dati mobili?» lo rifà dopo la risposta. `null` se non
 * c'è niente da chiedere.
 */
export const archivioDatiMobili = creaArchivio<(() => Promise<void>) | null>(null);

/**
 * Il percorso SVG dei moduli scuri di un QR: un quadratino `1×1` per modulo,
 * nelle coordinate dei moduli. Chi lo disegna mette il margine con il
 * `viewBox` (quattro moduli per lato, quel che la norma chiede).
 *
 * Una riga di moduli scuri di fila diventa un rettangolo solo: il percorso
 * pesa un terzo, e il disegno è lo stesso.
 */
export function percorsoQr(moduli: string, lato: number): string {
  if (lato <= 0 || moduli.length < lato * lato) return "";
  const pezzi: string[] = [];
  for (let y = 0; y < lato; y++) {
    let x = 0;
    while (x < lato) {
      if (moduli[y * lato + x] !== "1") {
        x++;
        continue;
      }
      const inizio = x;
      while (x < lato && moduli[y * lato + x] === "1") x++;
      pezzi.push(`M${inizio} ${y}h${x - inizio}v1h${inizio - x}z`);
    }
  }
  return pezzi.join("");
}

/** I dispositivi in vista: online prima, poi per nome; i revocati in fondo. */
export function ordinaDispositivi(
  dispositivi: readonly DispositivoLontano[],
): DispositivoLontano[] {
  return [...dispositivi].sort((a, b) => {
    if (a.revocato !== b.revocato) return a.revocato ? 1 : -1;
    if (a.online !== b.online) return a.online ? -1 : 1;
    return a.nome.localeCompare(b.nome);
  });
}

/**
 * Una riga dell'elenco unico della pagina «Dispositivi».
 *
 * I registri restano tre — i collegati via internet, i PC di «Ascolta
 * insieme», i dispositivi della sincronia — e si uniscono solo qui, nella
 * vista: un dispositivo collegato col QR porta con sé la sua riga di
 * «Ascolta insieme» (`insieme`) e il suo id della sincronia (`dispositivo`).
 */
export type RigaDispositivi =
  | {
      tipo: "lontano";
      lontano: DispositivoLontano;
      insieme: FidatoInsieme | null;
      sincronia: DispositivoSincronia | null;
    }
  | { tipo: "insieme"; insieme: FidatoInsieme }
  | { tipo: "sincronia"; sincronia: DispositivoSincronia };

/**
 * I tre elenchi in uno: prima i collegati via internet, ciascuno con le voci
 * degli altri due che sono lui; poi i PC di «Ascolta insieme» e i dispositivi
 * della sincronia che non sono nessuno di quelli. Questo dispositivo non c'è.
 */
export function unisciDispositivi(
  lontani: readonly DispositivoLontano[],
  fidati: readonly FidatoInsieme[],
  sincronia: readonly DispositivoSincronia[],
): RigaDispositivi[] {
  const fidatiPerId = new Map(fidati.map((f) => [f.id, f]));
  const altri = sincronia.filter((s) => !s.sonoIo);
  const sincroniaPerId = new Map(altri.map((s) => [s.id, s]));
  const fidatiUsati = new Set<number>();
  const sincroniaUsata = new Set<string>();
  const righe: RigaDispositivi[] = ordinaDispositivi(lontani).map((lontano) => {
    const insieme = lontano.insieme === null ? null : (fidatiPerId.get(lontano.insieme) ?? null);
    const suo = lontano.dispositivo === "" ? null : (sincroniaPerId.get(lontano.dispositivo) ?? null);
    if (insieme !== null) fidatiUsati.add(insieme.id);
    if (suo !== null) sincroniaUsata.add(suo.id);
    return { tipo: "lontano", lontano, insieme, sincronia: suo };
  });
  for (const insieme of ordinaFidati(fidati)) {
    if (!fidatiUsati.has(insieme.id)) righe.push({ tipo: "insieme", insieme });
  }
  for (const suo of altri) {
    if (!sincroniaUsata.has(suo.id)) righe.push({ tipo: "sincronia", sincronia: suo });
  }
  return righe;
}

/**
 * La scelta «il dispositivo con questa chiave» sul pannello, ricordata: chi la
 * legge si ridisegna solo quando **quel** dispositivo cambia (R13-11).
 *
 * `null` come chiave non sceglie niente, e il risultato è `null` per qualunque
 * pannello: `App` segue il dispositivo aperto anche quando non ce n'è nessuno, e
 * non deve ridisegnarsi a ogni `lontano:stato` — uno al minuto per dispositivo,
 * più i cambi di via e di presenza. Un oggetto nuovo con gli stessi campi tiene
 * l'identità di prima (`ugualiInSuperficie`: i campi sono tutti valori semplici).
 * Va rifatta quando cambia la chiave: la scelta ricordata guarda solo la
 * sorgente.
 */
export function sceltaDelDispositivo(
  chiave: string | null,
): (stato: StatoLontano | null) => DispositivoLontano | null {
  return sceltaRicordata(
    (stato: StatoLontano | null) =>
      chiave === null || stato === null
        ? null
        : (stato.dispositivi.find((d) => d.chiave === chiave) ?? null),
    (a, b) => a === b || (a !== null && b !== null && ugualiInSuperficie(a, b)),
  );
}

/** I dispositivi che si possono sfogliare adesso: online e non revocati. */
export function sfogliabili(stato: StatoLontano | null): DispositivoLontano[] {
  if (stato === null) return [];
  return ordinaDispositivi(stato.dispositivi.filter((d) => d.online && !d.revocato));
}

/**
 * «Scarica qui», com'è adesso: lo scrive `LontanoInApp` a ogni
 * `lontano:scarico`, lo legge la pagina «Sfoglia». `null` finché il nucleo non
 * ha risposto.
 */
export const archivioScarico = creaArchivio<ScaricoLontano | null>(null);

/** Una coda appena finita: c'era un brano in corso, e adesso non c'è niente. */
export function scaricoFinito(prima: ScaricoLontano | null, adesso: ScaricoLontano): boolean {
  return prima !== null && prima.titolo !== null && adesso.titolo === null && adesso.inCoda === 0;
}

/**
 * Il codice con cui «Scarica qui» rifiuta un file che non è musica: l'altro
 * dispositivo l'ha messo fra i brani con un'estensione che la scansione non
 * legge (`lontano/scarico.rs`, `estensione_audio`).
 */
export const NON_AUDIO = "fs.extensionRefused";

/** La coda è finita con un file rifiutato perché non era musica. */
export function scaricoConNonAudio(stato: ScaricoLontano): boolean {
  return stato.falliti > 0 && stato.errore === NON_AUDIO;
}

/**
 * La frase di fine coda di «Scarica qui»: la chiave e i valori, da dare a `t`.
 *
 * Il plurale lo sceglie solo `n`, e un secondo numero (`falliti`) non lo
 * sceglie: per questo con i brani arrivati il conto dei non riusciti sta in
 * coda alla frase («…; non riusciti: 2»), e con **nessun** brano arrivato la
 * frase è un'altra, il cui `n` sono proprio i non riusciti — non «Scaricati 0
 * brani» seguito da un conto che non accorda (R13-12).
 */
export function fraseScaricoFinito(
  adesso: Pick<ScaricoLontano, "fatti" | "falliti">,
): { chiave: Chiave; valori: Valori } {
  if (adesso.falliti === 0) return { chiave: "devices.download.done", valori: { n: adesso.fatti } };
  if (adesso.fatti === 0) return { chiave: "devices.download.allFailed", valori: { n: adesso.falliti } };
  return {
    chiave: "devices.download.doneWithErrors",
    valori: { n: adesso.fatti, falliti: adesso.falliti },
  };
}

/** Quanto del brano in corso è sceso, da 0 a 100. */
export function percentualeScarico(stato: ScaricoLontano): number {
  if (stato.totale <= 0) return 0;
  return Math.min(100, Math.floor((stato.byte / stato.totale) * 100));
}

/** Il codice di un errore che dice «sei sui dati mobili, e serve un sì». */
export const RETE_A_CONSUMO = "lontano.reteAConsumo";

/** L'errore è «sui dati mobili, chiedi»: il gesto aspetta una risposta. */
export function chiedeIDatiMobili(errore: unknown, stato: StatoLontano | null): boolean {
  return (
    typeof errore === "object" &&
    errore !== null &&
    (errore as Partial<ErroreIpc>).code === RETE_A_CONSUMO &&
    stato?.datiMobili === "chiedi"
  );
}

/**
 * Fa un gesto su un brano dell'altro. Se sui dati mobili serve un sì e
 * l'impostazione è «Chiedi», non lo dice come errore: lo mette da parte, e la
 * finestrella lo chiede (`LontanoInApp`). Con «No» l'errore passa, e dice
 * perché.
 */
export async function gestoLontano(
  fai: () => Promise<void>,
  onErrore: (errore: unknown) => void,
): Promise<void> {
  try {
    await fai();
  } catch (errore) {
    if (chiedeIDatiMobili(errore, archivioLontano.leggi())) {
      archivioDatiMobili.scrivi(() => fai);
      return;
    }
    onErrore(errore);
  }
}

/**
 * Il QR in vista non vale più: i dieci minuti sono passati. Il nucleo lo
 * toglie da sé al suo giro (e subito quando è usato o bruciato); questo serve
 * alla finestra per non mostrarlo nemmeno per quei secondi.
 */
export function qrScaduto(scadeMs: number, adesso: number): boolean {
  return scadeMs <= adesso;
}

/**
 * Il QR vale solo finché è a schermo: con un QR valido il nucleo tiene aperta
 * l'associazione (`rymembra/associa/1`) a chiunque lo presenti, e chi non lo
 * vede più non sa che c'è. La scheda che lo mostra lo annulla quando se ne va
 * — un'altra sezione, le Impostazioni chiuse — e annulla anche quello di un
 * «Mostra il QR» che torna quando lei non c'è più (il nucleo può metterci
 * qualche secondo: accende il nodo, aspetta il relay).
 *
 * `annulla` è `ipc.lontanoQrAnnulla`, passato da chi la usa: questo modulo
 * non carica l'IPC (vedi in cima).
 */
export function qrSoloInVista(annulla: () => Promise<StatoLontano>): {
  /** La scheda c'è: un QR che arriva si mostra. */
  monta: () => void;
  /** La scheda se ne va: il QR in vista non vale più. */
  smonta: () => void;
  /** Chiede un QR; arrivato a scheda chiusa, lo annulla. */
  crea: <T>(crea: () => Promise<T>) => Promise<T>;
} {
  let inVista = false;
  // Un QR arrivato a scheda aperta: il pannello lo dice solo col
  // `lontano:stato` che segue, e la scheda può andarsene prima.
  let creato = false;
  const togli = () => {
    comandoDelPannello(annulla)
      .catch(() => {
        /* Il nucleo lo toglie comunque alla scadenza: qui non c'è più una
           scheda a cui dire l'errore. */
      });
  };
  return {
    monta: () => {
      inVista = true;
    },
    smonta: () => {
      inVista = false;
      if (creato || archivioLontano.leggi()?.invito != null) togli();
      creato = false;
    },
    crea: async (crea) => {
      const fatto = await crea();
      if (inVista) creato = true;
      else togli();
      return fatto;
    },
  };
}

/**
 * I tentativi di collegamento della finestrella del link: ognuno ha il suo
 * numero, e «Smetti di aspettare» (o Esc) lo fa scadere. `nuovo` comincia un
 * tentativo e torna la domanda «è stato lasciato?», che resta vera anche dopo
 * che ne comincia un altro.
 */
export function tentativiDiCollegamento(): {
  nuovo: () => () => boolean;
  lascia: () => void;
} {
  let numero = 0;
  return {
    nuovo: () => {
      const mio = ++numero;
      return () => numero !== mio;
    },
    lascia: () => {
      numero++;
    },
  };
}

/**
 * «Accendi», poi «Collega»: il secondo solo se nel frattempo nessuno ha
 * lasciato la finestrella. `true` se il collegamento è partito.
 *
 * Il nucleo guarda gli annulli quando `lontano_collega` comincia: un «Smetti
 * di aspettare» premuto mentre `lontanoAttiva` girava — la finestrella diceva
 * già «Aspetto la conferma…» — arrivava prima, e il collegamento partiva lo
 * stesso, senza più niente da premere per fermarlo (reperto R13-2). Chi lascia
 * qui lo sa, e il collegamento non si chiede.
 */
export async function accendiPoiCollega(passi: {
  attiva: () => Promise<StatoLontano>;
  collega: () => void;
  annullato: () => boolean;
}): Promise<boolean> {
  await comandoDelPannello(passi.attiva);
  if (passi.annullato()) return false;
  passi.collega();
  return true;
}

/**
 * L'errore di un collegamento annullato da qui (`lontanoCollegaAnnulla`): non
 * è un guasto, e non si dice.
 */
export function annullatoDaQui(errore: unknown): boolean {
  return (
    typeof errore === "object" &&
    errore !== null &&
    (errore as Partial<ErroreIpc>).code === "internal.aborted"
  );
}

/** Che cosa chiude Indietro (o Escape) in «Sfoglia»: vedi [`passoIndietroSfoglia`]. */
export type PassoIndietroSfoglia = "album" | "artista" | "ricerca" | "scheda";

/**
 * Il prossimo livello di «Sfoglia» che Indietro chiude, o `null` se la pagina
 * non ha niente da chiudere e il gesto deve passare alla scala di `App`.
 *
 * Si chiude quel che **si vede**, un livello per volta, nell'ordine in cui ci
 * si è entrati: il disco, l'artista, la ricerca.
 *
 * - **Dispositivo che non risponde** (`online` falso): la pagina è lo stato
 *   vuoto «non risponde», i livelli non si disegnano, e chiuderli uno alla
 *   volta sarebbe un Indietro che a schermo non cambia niente (R13-6). Si esce.
 * - **Telefono**: dopo la ricerca si torna anche su «In evidenza» (`altraScheda`),
 *   e solo dopo si esce dalla pagina.
 * - **Computer** (`sulComputer`): Escape fa come nella libreria, che chiude il
 *   disco, l'artista e la ricerca ma non cambia mai la vista; la scheda non è un
 *   livello che si chiude con un tasto (R13-5).
 *
 * Pura, perché la scelta sta in un posto solo e si prova da sola; la pagina la
 * chiama a ogni disegno e si iscrive a `indietro.ts` solo se c'è un passo.
 */
export function passoIndietroSfoglia(stato: {
  online: boolean;
  album: boolean;
  artista: boolean;
  ricerca: boolean;
  altraScheda: boolean;
  sulComputer: boolean;
}): PassoIndietroSfoglia | null {
  if (!stato.online) return null;
  if (stato.album) return "album";
  if (stato.artista) return "artista";
  if (stato.ricerca) return "ricerca";
  if (stato.altraScheda && !stato.sulComputer) return "scheda";
  return null;
}

/** Il nome di una piattaforma, per l'icona e il testo. */
export function eTelefono(piattaforma: string): boolean {
  return piattaforma === "android";
}

/**
 * Il link di un QR, se il testo ne contiene uno: incollato si porta dietro
 * spazi, a capo, a volte il testo intorno di chi l'ha mandato.
 */
export function linkDa(testo: string): string | null {
  const trovato = /rymembra:\/\/collega\/[A-Za-z0-9_-]+/.exec(testo);
  return trovato === null ? null : trovato[0];
}

/** Quanto dura un brano dell'altro, per chi lo disegna come uno di qui. */
export function durataDi(brano: BranoRemoto): number {
  return Math.max(0, brano.durationMs);
}

/**
 * Il disco di un brano dell'altro, per aprirlo o metterlo in una scheda.
 *
 * Basta la chiave: è quella che `lontanoBraniAlbum` chiede. Il resto è quel
 * che il brano sa del suo disco, cioè quasi tutto tranne quanti brani ha:
 * `totalTracks` resta zero, e la scheda non scrive un numero che non sa.
 * L'artista è quello del brano, che per un disco a più voci è uno dei tanti.
 * `null` per un brano senza disco.
 */
export function albumDi(brano: BranoRemoto): Album | null {
  if (brano.albumKey === null) return null;
  return {
    albumKey: brano.albumKey,
    title: brano.album,
    artist: brano.artist,
    year: brano.year,
    genre: null,
    totalTracks: 0,
    coverArtHash: brano.coverArtHash,
  };
}

/**
 * I dischi dei brani appena entrati, al più `quanti`, nell'ordine in cui
 * compaiono.
 *
 * Il protocollo dà i brani aggiunti di recente, non i dischi: la stessa
 * ragione per cui la Home li raccoglie (`Home.tsx`, «Perché "aggiunti" sono
 * dischi»), perché un disco entra una cartella alla volta, e dodici brani
 * dello stesso disco sono la stessa copertina dodici volte.
 */
export function albumDaiBrani(brani: readonly BranoRemoto[], quanti: number): Album[] {
  const visti = new Set<string>();
  const album: Album[] = [];
  for (const brano of brani) {
    if (album.length >= quanti) break;
    const suo = albumDi(brano);
    if (suo === null || visti.has(suo.albumKey)) continue;
    visti.add(suo.albumKey);
    album.push(suo);
  }
  return album;
}

/** Le prime quattro copertine diverse di dei dischi: il mosaico di un artista. */
export function copertineDegliAlbum(album: readonly Album[]): string[] {
  const diverse = new Set<string>();
  for (const a of album) {
    if (diverse.size >= 4) break;
    if (a.coverArtHash !== null) diverse.add(a.coverArtHash);
  }
  return [...diverse];
}

/**
 * Una domanda per voce, `quanti` alla volta, con le risposte nell'ordine delle
 * voci.
 *
 * Per i brani di un artista, che arrivano un disco per domanda: tutte insieme
 * sarebbero cento domande in volo per una raccolta, e il nodo dell'altro le
 * servirebbe tutte prima di rispondere alla copertina che si sta guardando.
 */
export async function aGruppi<T, R>(
  voci: readonly T[],
  quanti: number,
  fai: (voce: T) => Promise<R>,
): Promise<R[]> {
  const passo = Math.max(1, quanti);
  const esiti: R[] = [];
  for (let da = 0; da < voci.length; da += passo) {
    esiti.push(...(await Promise.all(voci.slice(da, da + passo).map(fai))));
  }
  return esiti;
}

/** Quanto dura un elenco di brani dell'altro, per la testata di un disco. */
export function durataTotale(brani: readonly BranoRemoto[]): number {
  return brani.reduce((somma, brano) => somma + durataDi(brano), 0);
}
