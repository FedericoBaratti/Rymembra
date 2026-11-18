/**
 * Il collegamento via internet nella finestra: lo stato del pannello, il QR da
 * disegnare, e la domanda «usare i dati mobili?».
 *
 * # Un archivio, come «Ascolta insieme»
 *
 * Il pannello lo decide il nucleo e arriva intero con `lontano:stato` a ogni
 * cambiamento. Lo leggono la scheda delle Impostazioni, la barra laterale (i
 * dispositivi collegati) e la pagina Altro del telefono, i filtri della libreria e le righe:
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
import { creaArchivio } from "./archivio";
import { ordinaFidati } from "./insieme";
import { eventiEFotografia } from "./ultima";
import { dispositivoDellaRiga } from "./ipc/libreria";
import type { Chiave, Valori } from "./lingue";
import type {
  AnteprimaScarico,
  AvanzamentoSpecchio,
  DispositivoLontano,
  DispositivoSincronia,
  ErroreIpc,
  FidatoInsieme,
  LinkLontano,
  RichiestaLontano,
  StatoLontano,
  ScaricoLontano,
} from "./ipc";

/** Il pannello, `null` finché il nucleo non ha risposto. */
export const archivioLontano = creaArchivio<StatoLontano | null>(null);

/** Nessun dispositivo: un elenco fisso, che un selettore dell'archivio può dare senza rifarsi a ogni lettura. */
export const NESSUN_DISPOSITIVO: readonly DispositivoLontano[] = Object.freeze([]);

/**
 * Di chi è l'audio di un brano della libreria unica, dal suo percorso: `null`
 * se è di questa macchina, altrimenti il dispositivo che lo possiede — col nome
 * che ha nel pannello, o `nome: null` se non lo si conosce più (dimenticato).
 *
 * Serve a chi vuole fare, sull'audio, qualcosa che qui non si può: l'editor di
 * sincronizzazione non ne cerca gli attacchi (`Sincronizza.tsx`). Pura: il
 * percorso è quello di `Brano.path`, e la forma `lontano://<64hex>/<id>` la
 * riconosce `dispositivoDellaRiga`.
 */
export function proprietarioDellAudio(
  percorso: string | null | undefined,
  dispositivi: readonly DispositivoLontano[],
): { chiave: string; nome: string | null } | null {
  const chiave = dispositivoDellaRiga(percorso);
  if (chiave === null) return null;
  return { chiave, nome: dispositivi.find((d) => d.chiave === chiave)?.nome ?? null };
}

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
 * «Scarica qui», com'è adesso: lo scrive `LontanoInApp` a ogni
 * `lontano:scarico`, lo legge la striscia nella pila delle notifiche. `null` finché il nucleo non
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

/**
 * Da quanti brani «Scarica qui» chiede prima.
 *
 * Un album, una selezione, una manciata di brani si scaricano e basta: è un
 * gesto che chi lo fa vuole subito, e costa poco. Sopra questa soglia — un
 * artista con la discografia intera, una playlist da migliaia di righe — sono
 * decine di gigabyte sul disco e ore di rete di un altro dispositivo, e un clic
 * di troppo non si disfa a costo zero (F3c lo scrisse fra i rischi). Cinquanta è
 * anche la soglia con cui il nucleo chiede prima di togliere le righe di un
 * dispositivo (`SOGLIA_RIMOZIONI` in `specchio/archivio.rs`): un insieme «grande»
 * ha lo stesso senso nei due posti.
 */
export const SOGLIA_CONFERMA_SCARICO = 50;

/**
 * Quanti brani al più in una chiamata a `lontano_scarica_righe`: lo stesso
 * numero di `MASSIMI_PER_VOLTA` in `lontano/scarico.rs`, che rifiuta il resto.
 * Una prova (`confermaScarico.test.ts`) li tiene uguali.
 */
export const PER_VOLTA_SCARICO = 5000;

/** Un insieme di questa grandezza chiede conferma prima di scendere. */
export function chiedeConfermaPerLoScarico(brani: number): boolean {
  return brani >= SOGLIA_CONFERMA_SCARICO;
}

/** `elementi` a pezzi di `per` al più, nello stesso ordine. Un elenco vuoto non ne dà nessuno. */
export function inBlocchi<T>(elementi: readonly T[], per: number): T[][] {
  const blocchi: T[][] = [];
  for (let da = 0; da < elementi.length; da += per) blocchi.push(elementi.slice(da, da + per));
  return blocchi;
}

/**
 * La domanda prima di un «Scarica qui» grande: il titolo e la frase sotto, come
 * chiave e valori da dare a `t`.
 *
 * `formati` sono quelli della finestra (`brani_` e la misura in MB/GB): questo
 * modulo non carica le lingue. Lo spazio si dice solo se si sa:
 * - tutte le dimensioni note: «Occupano 8,2 GB»;
 * - qualcuna ignota: «Occupano almeno 3 GB» — la somma è un minimo, e dirla
 *   per intera sarebbe una cifra falsa;
 * - nessuna: la frase non dà una cifra. Una somma di zero byte non è «niente»,
 *   è che non lo si sa.
 */
export function domandaDelloScarico(
  anteprima: AnteprimaScarico,
  formati: { brani: (n: number) => string; peso: (byte: number) => string },
): {
  titolo: { chiave: Chiave; valori: Valori };
  descrizione: { chiave: Chiave; valori: Valori };
} {
  const titolo = {
    chiave: "devices.download.confirm.title" as const,
    valori: { brani: formati.brani(anteprima.brani) },
  };
  if (anteprima.byte <= 0) {
    return { titolo, descrizione: { chiave: "devices.download.confirm.bodyUnknown", valori: {} } };
  }
  const spazio = { spazio: formati.peso(anteprima.byte) };
  return {
    titolo,
    descrizione: {
      chiave:
        anteprima.senzaDimensione > 0
          ? "devices.download.confirm.bodyAtLeast"
          : "devices.download.confirm.body",
      valori: spazio,
    },
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

/** Il nome di una piattaforma, per l'icona e il testo. */
export function eTelefono(piattaforma: string): boolean {
  return piattaforma === "android";
}

/**
 * Le richieste di collegamento in fila: quella che si chiede adesso, e quante
 * aspettano dietro.
 *
 * Con più PC che leggono lo stesso link (o più link in giro) le domande
 * possono essere più d'una. La finestrella ne mostra una alla volta — un sì dato
 * alla cieca a un'altra sarebbe peggio — nell'ordine in cui sono arrivate; chi
 * aspetta dietro si dice, perché altrimenti sparirebbe finché non tocca a lui.
 */
export function richiesteInFila(richieste: readonly RichiestaLontano[]): {
  adesso: RichiestaLontano | null;
  dietro: number;
} {
  return { adesso: richieste[0] ?? null, dietro: Math.max(0, richieste.length - 1) };
}

/**
 * Quanto un collegamento resta «appena fatto»: il tempo che serve alla
 * persona per dare un nome, e il pannello per arrivare con la riga.
 */
export const FINESTRA_COLLEGATO_MS = 60_000;

/**
 * Il dispositivo appena collegato, se si riconosce.
 *
 * Chi mostrava il QR sente solo il nome che l'altro dichiara
 * (`lontano:collegato`); chi incollava il link ha il nome del link, se c'è, e la
 * risposta del comando con l'elenco. In tutti e due i casi la riga è quella con
 * quel nome collegata da meno di [`FINESTRA_COLLEGATO_MS`]; senza nome, l'ultima
 * collegata. Una riga revocata dall'altra parte non è un collegamento riuscito,
 * e una dal futuro — un orologio che è andato indietro — non vale come «adesso».
 */
export function appenaCollegato(
  dispositivi: readonly DispositivoLontano[],
  nome: string | null,
  adesso: number,
): DispositivoLontano | null {
  let trovato: DispositivoLontano | null = null;
  for (const d of dispositivi) {
    if (d.revocato || (nome !== null && d.nome !== nome)) continue;
    const eta = adesso - d.associatoMs;
    if (eta < 0 || eta > FINESTRA_COLLEGATO_MS) continue;
    if (trovato === null || d.associatoMs > trovato.associatoMs) trovato = d;
  }
  return trovato;
}

/**
 * Si propone di dare un nome a un dispositivo appena collegato solo se è un
 * computer: porta il nome del sistema (`DESKTOP-7K2L9QF`), che con due PC non
 * dice quale sia. Un telefono ha già quello che l'utente ha scelto in Android.
 */
export function proponiNome(dispositivo: DispositivoLontano): boolean {
  return !dispositivo.revocato && !eTelefono(dispositivo.piattaforma);
}

/**
 * Il link di un QR, se il testo ne contiene uno: incollato si porta dietro
 * spazi, a capo, a volte il testo intorno di chi l'ha mandato.
 */
export function linkDa(testo: string): string | null {
  const trovato = /rymembra:\/\/collega\/[A-Za-z0-9_-]+/.exec(testo);
  return trovato === null ? null : trovato[0];
}

// ── la libreria unica: il catalogo degli altri dispositivi ──────────────────

/** L'ultimo avanzamento del catalogo di ogni dispositivo, per chiave. */
export type AvanzamentiSpecchio = Readonly<Record<string, AvanzamentoSpecchio>>;

/**
 * Dove sta la lettura del catalogo di ciascun dispositivo.
 *
 * Lo scrive l'ascolto di `lontano:specchio` (`App`), e `lontanoSpecchioStato`
 * lo riempie per chi arriva dopo. Lo leggono la striscia «Leggo la libreria di
 * <PC>…» e la domanda sulle righe che un dispositivo non manda più.
 */
export const archivioSpecchio = creaArchivio<AvanzamentiSpecchio>({});

/** Gli avanzamenti con questo in più, o al posto di quello di prima. */
export function conAvanzamento(
  prima: AvanzamentiSpecchio,
  nuovo: AvanzamentoSpecchio,
): AvanzamentiSpecchio {
  return { ...prima, [nuovo.dispositivo]: nuovo };
}

/** Gli avanzamenti da una fotografia (`lontanoSpecchioStato`). */
export function daFotografia(elenco: readonly AvanzamentoSpecchio[]): AvanzamentiSpecchio {
  return Object.fromEntries(elenco.map((a) => [a.dispositivo, a]));
}

/** Quelli che stanno leggendo adesso. */
export function inLettura(avanzamenti: AvanzamentiSpecchio): AvanzamentoSpecchio[] {
  return Object.values(avanzamenti).filter((a) => a.fase === "leggo");
}

/** Quelli che non sanno servire il catalogo: una versione di Rymembra di prima. */
export function nonSupportati(avanzamenti: AvanzamentiSpecchio): AvanzamentoSpecchio[] {
  return Object.values(avanzamenti).filter((a) => a.fase === "nonSupportato");
}

/** Quelli con righe che non mandano più, e che aspettano un sì per andarsene. */
export function daConfermare(avanzamenti: AvanzamentiSpecchio): AvanzamentoSpecchio[] {
  return Object.values(avanzamenti).filter((a) => a.fase === "fine" && a.inAttesa > 0);
}

/**
 * Il dispositivo di cui chiedere adesso «toglierli?», se ce n'è uno.
 *
 * `lasciate` è quel che il nucleo ricorda: per dispositivo, di quante righe la
 * persona ha detto «Lasciali». Il nucleo rifà il conto a ogni giro — ogni cinque
 * minuti — e la stessa domanda alla stessa cifra tornerebbe ogni cinque minuti,
 * e a ogni riavvio: si ricorda finché il numero non cambia, e un numero diverso
 * è una situazione diversa che si chiede di nuovo.
 *
 * `null` è «il nucleo non ha ancora risposto»: non si chiede niente. Partire da
 * una mappa vuota farebbe lampeggiare, a ogni avvio, una domanda già risolta.
 *
 * Il ricordo **non** si dimentica da solo quando le righe tornano (un giro finito
 * con `inAttesa` a zero): quello zero non è un segnale di cui fidarsi. Un giro
 * che non riesce a leggere, o a togliere le righe scadute, finisce lo stesso
 * `fine` con zero in attesa (`togli_le_scadute` in `lontano/specchio.rs` parte da
 * zero e un guasto non rompe il giro), e il ricordo si perderebbe per un guasto:
 * la domanda tornerebbe alla stessa cifra, che è proprio quel che il ricordo
 * evita. Si dimentica con «Togli» e con «Dimentica», che sono gesti.
 */
export function domandaSullePerdite(
  avanzamenti: AvanzamentiSpecchio,
  nomi: ReadonlyMap<string, string>,
  lasciate: ReadonlyMap<string, number> | null,
): AvanzamentoSpecchio | undefined {
  if (lasciate === null) return undefined;
  return daConfermare(avanzamenti).find(
    (a) => nomi.has(a.dispositivo) && lasciate.get(a.dispositivo) !== a.inAttesa,
  );
}

/** Ogni quanto, al più, la libreria si rilegge mentre un catalogo sta arrivando. */
export const RILEGGI_DURANTE_LA_LETTURA_MS = 3_000;

/**
 * Dopo questo avanzamento gli elenchi vanno riletti?
 *
 * Il giro del catalogo scrive in `tracks` e non manda `cartelle:cambiate`: le
 * liste non lo saprebbero. A giro finito si rileggono se è cambiato qualcosa —
 * c'era una lettura in corso, o il numero di righe non è più quello —, e
 * durante la lettura ogni tanto, perché la libreria si usa mentre arriva: il
 * primo collegamento di una libreria da cinquantamila brani dura qualche minuto.
 */
export function rileggereDopo(
  prima: AvanzamentoSpecchio | undefined,
  ora: AvanzamentoSpecchio,
  adesso: number,
  ultimaRilettura: number,
): boolean {
  if (ora.fase === "leggo") {
    return ora.fatti > (prima?.fatti ?? 0) && adesso - ultimaRilettura >= RILEGGI_DURANTE_LA_LETTURA_MS;
  }
  if (ora.fase === "fine") {
    return prima?.fase === "leggo" || prima?.righe !== ora.righe;
  }
  return false;
}

/**
 * Fa un gesto che apre dei brani di un altro dispositivo, e se sui dati mobili
 * serve un sì lo mette da parte: la finestrella «Usare i dati mobili?» lo rifà
 * dopo la risposta (`LontanoInApp`).
 *
 * Come [`gestoLontano`], ma gli altri errori **salgono**: chi suona una lista
 * dalla libreria (`suonaDa`) li lascia a chi l'ha chiamata, che li dice con la
 * sua fascia. Con «No» sui dati mobili l'errore sale e dice perché.
 */
export async function conIlSiDeiDatiMobili(fai: () => Promise<void>): Promise<void> {
  try {
    await fai();
  } catch (errore) {
    if (!chiedeIDatiMobili(errore, archivioLontano.leggi())) throw errore;
    archivioDatiMobili.scrivi(() => fai);
  }
}
