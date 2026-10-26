/**
 * «Ascolta insieme», le regole che la finestra applica da sé.
 *
 * # Perché un modulo puro
 *
 * Quel che si vede di una sessione lo decide il nucleo, e arriva fatto con
 * `insieme:stato`: chi c'è, chi ospita, chi è associato. Restano però delle
 * scelte che toccano alla finestra — in che ordine si elencano i PC, quando un
 * invito non vale più, come si scrive un codice perché lo si confronti a colpo
 * d'occhio, quale indirizzo scritto a mano è buono — e sono proprio quelle che
 * si sbagliano senza che nessuno se ne accorga: un invito scaduto da dieci
 * secondi con il tasto «Accetta» ancora acceso, un `192.168.1.300` mandato al
 * nucleo e rifiutato con un errore generico. Qui stanno in funzioni senza
 * React, senza lingue e senza nucleo, e `test/insieme.test.ts` le prova una
 * per una con `node --test`.
 *
 * Per la stessa ragione da `./ipc` arrivano solo i **tipi**: `ipc.ts` porta con
 * sé le lingue, che si caricano con `import.meta.glob` e fuori da Vite non
 * esistono. I testi qui sono chiavi (`Chiave`), e le traduce chi disegna.
 *
 * # Le costanti ripetute
 *
 * La porta, il tetto degli indirizzi e quello del nome sono scritti anche nel
 * nucleo (`rymembra_insieme::PORTA`, `rymembra_app::insieme`). Ripeterli è il
 * male minore: chiederli al nucleo vorrebbe dire una chiamata per sapere quanto
 * può essere lungo un campo di testo, e il nucleo li controlla comunque da sé
 * — qui servono a dire «no» prima del viaggio, non a decidere.
 */
import type {
  Brano,
  FidatoInsieme,
  InsiemeNelLettore,
  ProprietarioInsieme,
  StatoInsieme,
  VicinoInsieme,
} from "./ipc";
import type { Chiave } from "./lingue";

/** La porta su cui si ospita, quando un indirizzo scritto a mano non la dice. */
export const PORTA = 47811;

/** Quanti indirizzi si possono scrivere a mano. Gli stessi del nucleo. */
export const MASSIMO_INDIRIZZI = 16;

/** Quanto può essere lungo il nome di un PC, in caratteri. */
export const MASSIMO_NOME = 64;

/**
 * Fin dove arriva il cursore della latenza nella calibrazione, in ms.
 *
 * Trecento e non i cinquecento delle Impostazioni: la calibrazione si fa a
 * orecchio, un millisecondo alla volta, e trecento bastano anche alle cuffie
 * Bluetooth più lente. Una corsa più lunga vorrebbe dire un cursore dove un
 * pixel vale due millisecondi, cioè più grossolano proprio nel posto in cui
 * serve finezza.
 */
export const LIMITE_CALIBRAZIONE_MS = 300;

// ── id ─────────────────────────────────────────────────────────────────────

/**
 * L'id è una riga della libreria di **questo** PC.
 *
 * La regola è una sola, la stessa del nucleo (`riproduzione/ospiti.rs::chi`):
 * maggiore di zero è libreria, minore di zero è una voce della sessione — un
 * brano che sta in coda perché la sessione ce l'ha messo, e che su questo PC
 * può non esistere affatto. Su una voce i gesti di libreria — cuore, stelle,
 * playlist, «mostra nella cartella», il testo da cercare — scriverebbero su una
 * riga che non c'è, o peggio sulla riga che ha per caso lo stesso numero
 * cambiato di segno. Chi disegna un gesto di libreria chiede qui prima.
 */
export function idDiLibreria(id: number): boolean {
  return Number.isInteger(id) && id > 0;
}

/**
 * Le righe della coda da chiedere al nucleo: quelle che non sono in mano, più
 * tutte le voci della sessione quando le loro descrizioni sono cambiate.
 *
 * La coda tiene le righe per id e chiede solo quelle nuove (`useRigheCoda`).
 * Per la libreria va bene: titolo, artista e copertina di un brano non
 * cambiano mentre sta in coda. Per una voce della sessione no — arriva prima
 * con quel che si sa e poi si completa: la copertina viaggia dopo, la durata la
 * scopre chi la legge — e l'id resta lo stesso, quindi la riga restava quella
 * del primo istante, senza copertina, per sempre. `vociStantie` è il segno del
 * nucleo (`InsiemeNelLettore.revisione`) che qualcosa è cambiato: allora si
 * richiedono le voci, e solo loro — le righe della libreria restano in mano.
 * Ogni id una volta sola, anche se la coda lo nomina due volte.
 */
export function righeDaChiedere(
  ids: readonly number[],
  inMano: (id: number) => boolean,
  vociStantie: boolean,
): number[] {
  return [...new Set(ids)].filter((id) => !inMano(id) || (vociStantie && !idDiLibreria(id)));
}

// ── il codice dell'associazione ─────────────────────────────────────────────

/**
 * Il codice di quattro cifre, come si legge: `"4821"` → `"48 21"`.
 *
 * Due coppie perché è così che si confrontano due schermi: l'occhio salta da
 * uno all'altro e ritrova il segno su un gruppo di due cifre, non su una fila
 * di quattro. Quel che non è un codice di quattro cifre resta com'è: è il
 * nucleo a dire cos'è un codice, e un testo cambiato qui sarebbe un codice
 * diverso da quello sull'altro schermo.
 */
export function codiceLeggibile(codice: string): string {
  const pulito = codice.trim();
  return /^\d{4}$/.test(pulito) ? `${pulito.slice(0, 2)} ${pulito.slice(2)}` : pulito;
}

/**
 * Il codice per chi ascolta lo schermo: `"4821"` → `"4 8 2 1"`.
 *
 * «48 21» letto da una sintesi vocale diventa «quarantotto ventuno», e chi
 * confronta a voce con l'altro PC deve tradurre di nuovo in cifre. Una cifra
 * per volta si confronta come la si vede.
 */
export function codiceDaLeggere(codice: string): string {
  return [...codice.trim()].filter((c) => c !== " ").join(" ");
}

// ── inviti, richieste, scadenze ─────────────────────────────────────────────

/**
 * Quanti secondi mancano a una scadenza, arrotondati per eccesso.
 *
 * Per eccesso: a 59,2 secondi dalla fine si scrive 60 e non 59, così lo zero
 * compare solo quando è davvero finita — un conto alla rovescia fermo su «0 s»
 * con il tasto ancora premibile racconterebbe una cosa falsa per quasi un
 * secondo. Mai negativo.
 *
 * `scadeMs` è un orologio di parete di **questo** PC (il nucleo lo calcola
 * qui, all'arrivo dell'invito), quindi si confronta con `Date.now()` e non con
 * `performance.now()`.
 */
export function secondiRestanti(scadeMs: number, adessoMs: number): number {
  return Math.max(0, Math.ceil((scadeMs - adessoMs) / 1000));
}

/**
 * Gli inviti e le richieste che valgono ancora.
 *
 * Il nucleo toglie quelli scaduti al passo successivo del suo filo, cioè un
 * poco dopo: fra i due istanti un «Accetta» premuto arriverebbe a un invito
 * che di là non esiste più. Si nascondono qui al secondo, e il nucleo resta
 * quello che decide.
 */
export function ancoraValidi<T extends { scadeMs: number }>(
  elenco: readonly T[],
  adessoMs: number,
): T[] {
  return elenco.filter((voce) => voce.scadeMs > adessoMs);
}

// ── ordinamenti ────────────────────────────────────────────────────────────

/** Due nomi, nell'ordine in cui si leggono: senza maiuscole, coi numeri da numeri. */
function perNome(a: string | null, b: string | null): number {
  // Il PC che non ha ancora detto come si chiama va in fondo: un elenco che
  // comincia con «PC senza nome» mette per primo quello di cui si sa meno.
  if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1;
  return a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });
}

/** `true` prima di `false`. */
function primaISi(a: boolean, b: boolean): number {
  return a === b ? 0 : a ? -1 : 1;
}

/**
 * I PC che ospitano, visti in rete, nell'ordine in cui si cercano.
 *
 * Prima quelli con cui si può fare qualcosa: i bloccati in fondo, perché per
 * loro non c'è un tasto; fra gli altri prima gli associati, che si raggiungono
 * con un clic, poi gli sconosciuti, che chiedono un codice. Dentro ogni gruppo
 * per nome, e a parità di nome per impronta: due PC che si chiamano entrambi
 * «UFFICIO» devono restare nello stesso ordine fra un giro di scoperta e
 * l'altro, o chi sta per premere si troverebbe sotto il dito l'altro.
 */
export function ordinaVicini(vicini: readonly VicinoInsieme[]): VicinoInsieme[] {
  return [...vicini].sort(
    (a, b) =>
      primaISi(!a.bloccato, !b.bloccato) ||
      primaISi(a.pari !== null, b.pari !== null) ||
      perNome(a.nome, b.nome) ||
      a.impronta.localeCompare(b.impronta),
  );
}

/**
 * I PC associati, nell'ordine in cui servono.
 *
 * Quelli della sessione in corso, poi quelli che ospitano, poi i collegati,
 * poi gli altri; i bloccati in fondo. È l'ordine della domanda che si fa
 * aprendo l'elenco — «chi c'è adesso?» — e non quello in cui sono stati
 * associati, che è una data che non interessa a nessuno.
 */
export function ordinaFidati(fidati: readonly FidatoInsieme[]): FidatoInsieme[] {
  return [...fidati].sort(
    (a, b) =>
      primaISi(!a.bloccato, !b.bloccato) ||
      primaISi(a.inSessione, b.inSessione) ||
      primaISi(a.ospita, b.ospita) ||
      primaISi(a.presente, b.presente) ||
      perNome(a.nome, b.nome) ||
      a.id - b.id,
  );
}

/**
 * I PC che si possono invitare, per chi ospita.
 *
 * Solo i collegati: l'invito viaggia sulla connessione di presenza, e un PC che
 * non è collegato non lo riceverebbe — il tasto ci sarebbe, il clic finirebbe
 * in un errore. Non chi è già dentro, e non i bloccati.
 */
export function daInvitare(stato: StatoInsieme): FidatoInsieme[] {
  return ordinaFidati(
    stato.fidati.filter((f) => f.presente && !f.inSessione && !f.bloccato),
  );
}

/**
 * I PC che ospitano, per chi cerca una sessione: quelli visti in rete, più gli
 * associati che ospitano e la scoperta non ha visto.
 *
 * Il secondo gruppo esiste per le reti dove il broadcast non passa: un PC
 * raggiunto con un indirizzo scritto a mano può essere collegato e ospitare
 * senza aver mai risposto a una domanda di scoperta. Un associato già fra i
 * vicini non si ripete. La `chiave` di un associato aggiunto così resta vuota:
 * serve solo ad associare, e con un associato non si associa — si chiede.
 *
 * Un vicino associato resta solo se il suo associato dice che ospita adesso
 * (`presente && ospita`). La scoperta ricorda chi ha risposto per qualche giro
 * ancora dopo l'ultima volta, e un PC che ha appena chiuso la sessione restava
 * in elenco con «Chiedi di unirti» acceso: il clic finiva in un errore. La
 * connessione di presenza invece lo sa subito, ed è anche la strada su cui la
 * richiesta viaggerebbe — un associato non collegato non la riceverebbe
 * comunque. I bloccati restano dove sono, in fondo e senza tasto: con loro non
 * c'è connessione, quindi `presente` per loro non è mai vero, e toglierli
 * vorrebbe dire nascondere proprio il motivo per cui quel PC non risponde.
 */
export function ospitanti(stato: StatoInsieme): VicinoInsieme[] {
  const fidati = new Map(stato.fidati.map((f) => [f.id, f]));
  const vivi = stato.vicini.filter((v) => {
    if (v.pari === null || v.bloccato) return true;
    const fidato = fidati.get(v.pari);
    return fidato !== undefined && fidato.presente && fidato.ospita;
  });
  const visti = new Set(stato.vicini.flatMap((v) => (v.pari === null ? [] : [v.pari])));
  const daiFidati: VicinoInsieme[] = stato.fidati
    .filter((f) => f.ospita && f.presente && !visti.has(f.id))
    .map((f) => ({
      chiave: "",
      impronta: f.impronta,
      nome: f.nome,
      pari: f.id,
      bloccato: f.bloccato,
    }));
  return ordinaVicini([...vivi, ...daiFidati]);
}

/**
 * Gli associati che ospitano adesso, e da cui si può chiedere di entrare.
 *
 * Servono in due momenti in cui chi guarda sta per fare la cosa sbagliata
 * senza saperlo: quando preme «Ospita una sessione» mentre un collega ospita
 * già — due sessioni separate nella stessa stanza, che è quasi sempre un
 * equivoco — e quando ha appena finito di associarsi a un PC che ospita, e il
 * gesto che cerca è entrare, non tornare a cercare «Chiedi di unirti». Non i
 * bloccati, e non chi è già nella sessione di questo PC.
 */
export function fidatiCheOspitano(stato: StatoInsieme): FidatoInsieme[] {
  return ordinaFidati(
    stato.fidati.filter((f) => f.presente && f.ospita && !f.bloccato && !f.inSessione),
  );
}

/**
 * Gli inviti mandati che valgono ancora, per pari: l'ora in cui il tasto
 * «Invita» torna acceso.
 *
 * Il nucleo non elenca gli inviti mandati, solo quelli ricevuti, quindi il
 * minuto di attesa lo tiene la finestra. Ma lo tiene **appoggiato** a quel che
 * il nucleo dice: un invito non vale più se è scaduto, se la sessione non è
 * più di questo PC, o se il suo destinatario non si può più invitare — è
 * entrato, si è scollegato, è stato bloccato. Senza questa potatura un tasto
 * restava spento per un minuto per una persona già dentro, e il conto di
 * quelli in attesa non scendeva mai a zero.
 */
export function invitiAncoraAperti(
  invitati: ReadonlyMap<number, number>,
  stato: StatoInsieme | null,
  adessoMs: number,
): ReadonlyMap<number, number> {
  if (invitati.size === 0) return invitati;
  if (stato === null || stato.fase !== "ospita") return new Map();
  const invitabili = new Set(daInvitare(stato).map((f) => f.id));
  const restano = [...invitati].filter(
    ([pari, scadeMs]) => scadeMs > adessoMs && invitabili.has(pari),
  );
  return restano.length === invitati.size ? invitati : new Map(restano);
}

// ── perché non si vede niente ───────────────────────────────────────────────

/**
 * Le cose da controllare quando non si vede il PC che ospita, in ordine.
 *
 * Una lista e non una frase, perché la causa non si può sapere da qui: chi
 * segue non apre porte e non riceve niente finché l'host non risponde, e un
 * host dietro un firewall che scarta in silenzio è identico a un host spento.
 * L'ordine è quello della probabilità sulle reti di un ufficio: il firewall
 * dell'host prima di tutto — è la richiesta di Windows a cui si risponde in
 * fretta, ed è su una rete Pubblica che si sbaglia — poi la rete, poi gli
 * indirizzi a mano, e per ultima la versione, che di solito si sa già.
 *
 * Il punto sugli indirizzi cambia se se n'è già scritto qualcuno: suggerire di
 * scriverne uno a chi l'ha appena fatto sarebbe un consiglio che non ascolta.
 */
export function causeIrraggiungibile(indirizziAMano: number): Chiave[] {
  return [
    "together.causes.hostOn",
    "together.causes.hostFirewall",
    "together.causes.sameNetwork",
    indirizziAMano > 0
      ? "together.causes.addresses.some"
      : "together.causes.addresses.none",
    "together.causes.version",
  ];
}

/**
 * La frase dell'elenco vuoto: «per ora» nei primi secondi, poi cosa
 * controllare.
 *
 * Nei primi secondi un elenco vuoto è normale — il giro di scoperta è appena
 * partito — e dire subito «controlla il firewall» farebbe pensare a un guasto
 * che non c'è. Passato quel tempo invece «per ora» non spiega più niente, e
 * chi guarda è proprio chi non sa dove mettere le mani: al primo vero
 * tentativo fra due PC la persona ha acceso la funzione, ha aspettato, e non ha
 * trovato né l'host né il perché. Le tre cose più probabili si dicono nella
 * frase stessa, senza aprire l'elenco delle cause; l'ultima cambia se degli
 * indirizzi a mano ci sono già, come in [`causeIrraggiungibile`].
 */
export function fraseNessunVicino(trascorso: boolean, indirizziAMano: number): Chiave {
  if (!trascorso) return "together.nearby.none";
  return indirizziAMano > 0
    ? "together.nearby.none.still.addresses"
    : "together.nearby.none.still";
}

/**
 * La frase sul profilo di rete, per chi ospita o per chi segue.
 *
 * Dice due cose diverse ai due, ed è il motivo per cui è una funzione: chi
 * ospita è l'unico che apre una porta, quindi l'unico a cui Windows chiede
 * qualcosa, e l'unico per cui una rete classificata «Pubblica» cambia le cose.
 * Chi segue non apre niente: dirgli di toccare il firewall gli farebbe aprire
 * una regola che non serve, su un PC che non ne ha bisogno.
 */
export function fraseProfiloRete(ospita: boolean): Chiave {
  return ospita ? "together.network.host" : "together.network.member";
}

// ── il segno della sessione ─────────────────────────────────────────────────

/** Un pezzo di frase: la chiave, e i valori che ci vanno dentro. */
export interface Pezzo {
  chiave: Chiave;
  valori?: Record<string, string | number>;
}

/**
 * Chi c'è: «In ascolto con Marco · 3 persone», o «Ospiti tu · 3 persone».
 *
 * Il numero comprende l'host, come `persone` del nucleo: è la risposta a
 * «quanti stiamo sentendo questa musica», e chi ospita la sta sentendo.
 *
 * Prende i soli tre campi che usa perché lo chiamano due fonti: il segno
 * accanto a quel che suona (`StatoRiproduzione.insieme`) e la fascia della
 * sessione, che legge `StatoInsieme.sessione` e c'è anche quando non suona
 * niente.
 */
export function pezzoSessione(
  insieme: Pick<InsiemeNelLettore, "ospita" | "host" | "persone">,
): Pezzo {
  return insieme.ospita
    ? { chiave: "together.sign.hosting", valori: { n: insieme.persone } }
    : {
        chiave: "together.sign.with",
        valori: { nome: insieme.host, n: insieme.persone },
      };
}

/**
 * Di chi è quel che suona, e da dove arrivano i byte.
 *
 * Niente quando la voce è di questo PC: per chi ospita è quasi sempre così, e
 * «da questo PC» scritto sotto ogni brano sarebbe una riga che non dice niente.
 * Una voce di un altro: «in streaming da Marco», oppure — se se ne è trovata
 * una copia identica qui (fase 3) — «di Marco · da file locale», che è la
 * differenza che spiega perché quel brano non si interrompe se la rete va giù.
 *
 * «Aggancio…» finché chi segue non è a regime: è il momento in cui due casse
 * nella stessa stanza si sentono sfasate, e chi le sente deve sapere che è
 * previsto e passa da sé. Chi ospita è sempre agganciato: è lui l'orologio.
 */
export function pezziVoce(insieme: InsiemeNelLettore): Pezzo[] {
  const pezzi: Pezzo[] = [];
  const di = insieme.proprietario;
  if (di !== null) {
    if (insieme.locale) {
      pezzi.push({ chiave: "together.sign.owner", valori: { nome: di } });
      pezzi.push({ chiave: "together.sign.local" });
    } else {
      pezzi.push({ chiave: "together.sign.streaming", valori: { nome: di } });
    }
  }
  if (!insieme.ospita && !insieme.agganciato) {
    pezzi.push({ chiave: "together.sign.locking" });
  }
  return pezzi;
}

/**
 * La riga intera del segno, nell'ordine in cui si legge quando non ci sta.
 *
 * La riga finisce nei puntini: sotto il cursore della barra e sotto l'album
 * nella colonna lo spazio è quello che è, e il nome di un PC può essere lungo
 * quanto vuole. Con l'ordine di prima — chi, poi di chi è la voce, poi
 * «aggancio…» — i puntini mangiavano proprio l'ultima parte, che è l'unica che
 * spiega un suono sfasato **adesso**. Quindi l'aggancio, quando c'è, va in
 * testa; il resto segue nell'ordine di [`pezzoSessione`] e [`pezziVoce`], e la
 * frase intera resta nel `title` per chi passa sopra col puntatore.
 */
export function pezziSegno(insieme: InsiemeNelLettore): Pezzo[] {
  const voce = pezziVoce(insieme);
  const urgenti = voce.filter((p) => p.chiave === "together.sign.locking");
  const resto = voce.filter((p) => p.chiave !== "together.sign.locking");
  return [...urgenti, pezzoSessione(insieme), ...resto];
}

// ── i gesti in sessione ─────────────────────────────────────────────────────

/**
 * I gesti del lettore che una sessione può spegnere, per famiglie.
 *
 * - `trasporto`: pausa, ripresa, avanti, indietro, salto, ripeti;
 * - `coda`: far partire una riga, toglierla, spostarla, e aggiungerne
 *   (suona, dopo, in coda);
 * - `svuota`: svuotare la coda;
 * - `mescola`: il casuale;
 * - `ripristina`: l'«Annulla» di una coda sostituita;
 * - `radio`, `autoplay`, `timer`: la radio da un brano, la coda che continua
 *   da sola, il timer di spegnimento.
 */
export type GestoInSessione =
  | "trasporto"
  | "coda"
  | "svuota"
  | "mescola"
  | "ripristina"
  | "radio"
  | "autoplay"
  | "timer";

/**
 * Il gesto fa qualcosa, adesso?
 *
 * È la regola di `insieme/strada.rs` vista dalla finestra, e serve a spegnere
 * i tasti **prima** del clic invece di rispondere al clic con un errore: un
 * tasto acceso che risponde «non si può» insegna che il lettore è rotto.
 *
 * - Fuori sessione tutto si può.
 * - Il casuale mai, per nessuno: cambierebbe l'ordine per tutti senza che gli
 *   altri lo vedano.
 * - L'«Annulla» di una coda sostituita mai, per nessuno: rimetterebbe una
 *   coda di prima della sessione, fatta di brani che gli altri non hanno.
 * - La radio, l'autoplay e il timer mai, per nessuno: i primi due
 *   riempirebbero la coda di tutti con brani che nessuno ha scelto, il terzo
 *   fermerebbe la musica a tutti per l'ora di andare a letto di uno.
 * - Svuotare la coda solo chi ospita: chi segue la toglierebbe a tutti.
 * - Trasporto e coda secondo i `diritti` che chi ospita ha concesso: il
 *   trasporto da `comanda` in su, la coda solo con `tutto`. Chi ospita ha
 *   sempre `tutto`; per chi segue funzionano come per l'host, l'ordine passa
 *   dall'host invece che da qui.
 *
 * Il nucleo decide comunque da sé: questa è la cortesia, la porta è là — e
 * la porta vera è l'host, che ricontrolla ogni gesto all'arrivo.
 */
export function gestoPermesso(
  insieme: InsiemeNelLettore | null,
  gesto: GestoInSessione,
): boolean {
  if (insieme === null) return true;
  switch (gesto) {
    case "mescola":
    case "ripristina":
    case "radio":
    case "autoplay":
    case "timer":
      return false;
    case "svuota":
      return insieme.ospita;
    case "trasporto":
      return insieme.diritti !== "ascolta";
    case "coda":
      return insieme.diritti === "tutto";
  }
}

/**
 * Le voci della coda che un membro ha aggiunto, per maniglia.
 *
 * Il nucleo le manda come elenco (`SessioneInsieme.proprietari`) e la coda le
 * cerca riga per riga: una mappa fa di ogni ricerca un passo solo, anche su
 * una coda di mille voci.
 */
export function mappaProprietari(
  proprietari: readonly ProprietarioInsieme[],
): ReadonlyMap<number, string> {
  return new Map(proprietari.map((p) => [p.id, p.nome]));
}

/**
 * Di chi è la voce di una riga della coda: «di Marco».
 *
 * Solo per le voci che un **membro** ha aggiunto e che non sono di questo PC,
 * che sono le sole che il nucleo elenca: per chi ospita le voci dei membri,
 * per chi segue quelle degli altri membri. Le voci di chi ospita non portano
 * l'etichetta: sono la coda stessa, e la fascia della sessione dice già di chi
 * è. Una riga della libreria non ha proprietario
 * — è di chi la guarda — e non si cerca nemmeno: una maniglia e un id di
 * libreria non possono coincidere, ma è meglio che la regola lo dica.
 */
export function proprietarioDellaRiga(
  id: number,
  proprietari: ReadonlyMap<number, string>,
): string | null {
  if (idDiLibreria(id)) return null;
  return proprietari.get(id) ?? null;
}

/**
 * Il brano da disegnare, con il titolo della voce di calibrazione tradotto.
 *
 * La voce di calibrazione nasce nel nucleo con un titolo italiano fisso — è la
 * stessa su ogni PC, e il nucleo di lingue non ne conosce — e la finestra la
 * riconosce da `calibrazione` e la scrive nella lingua di chi guarda. Il terzo
 * campo, che per un brano è l'album, porta l'istruzione: la riga sotto il
 * titolo è il posto dove chi calibra guarda mentre ascolta. Tutto il resto
 * del brano — id, copertina, durata — resta quel che il nucleo ha mandato.
 *
 * `tr` è la funzione che traduce, passata da fuori come in `menuBrani.ts`:
 * così questo modulo resta senza lingue, e la prova senza `import.meta`.
 */
export function branoDaMostrare<B extends Pick<Brano, "title" | "artist" | "album">>(
  brano: B,
  insieme: InsiemeNelLettore | null,
  tr: (chiave: Chiave) => string,
): B {
  if (insieme === null || !insieme.calibrazione) return brano;
  return {
    ...brano,
    title: tr("together.calibrate.voice.title"),
    artist: tr("together.calibrate.voice.artist"),
    album: tr("together.calibrate.voice.album"),
  };
}

// ── avvisi ─────────────────────────────────────────────────────────────────

/**
 * La chiave di un avviso arrivato dal nucleo, se è una che si può mostrare.
 *
 * Il nucleo manda la **chiave** e non la frase (`insieme:avviso`), e la
 * finestra la traduce. Si accettano solo le due famiglie che il contratto
 * prevede — `together.notice.*` e `errors.insieme.*` — e solo se è fatta come
 * una chiave: il nome di un PC viaggia accanto, come valore, e non deve mai
 * poter diventare la chiave stessa e finire a schermo così com'è.
 */
export function chiaveAvviso(chiave: string): string | null {
  if (!/^[A-Za-z0-9.]+$/.test(chiave)) return null;
  return chiave.startsWith("together.notice.") || chiave.startsWith("errors.insieme.")
    ? chiave
    : null;
}

/** L'avviso è un guasto (rosso, `alert`) e non una notizia. */
export function avvisoEGuasto(chiave: string): boolean {
  return chiave.startsWith("errors.");
}

// ── nomi e indirizzi ────────────────────────────────────────────────────────

/**
 * Il nome di un PC com'è giusto mandarlo: senza caratteri di controllo, con
 * gli spazi ridotti a uno, tagliato a [`MASSIMO_NOME`] caratteri.
 *
 * Il nucleo lo ripulisce di nuovo (`rymembra_app::insieme::salva_nome`): qui
 * serve a non mandare un nome che di là tornerebbe diverso da come lo si vede
 * nel campo. Tagliato per **caratteri**, non per unità UTF-16: un'emoji a
 * cavallo del limite spezzata a metà sarebbe un carattere che non esiste.
 */
export function nomePulito(testo: string): string {
  // Il carattere di controllo si sceglie per categoria Unicode e non con una
  // classe scritta a mano: `\p{Cc}` è proprio «controllo», tabulazioni e a capo
  // compresi, e un ritorno a capo incollato da un altro programma è il caso
  // che capita davvero.
  const senza = testo.replace(/\p{Cc}/gu, " ").replace(/\s+/g, " ").trim();
  return [...senza].slice(0, MASSIMO_NOME).join("").trim();
}

/**
 * Un indirizzo scritto a mano, nella forma che il nucleo salva: `a.b.c.d:porta`.
 *
 * IPv4 soltanto, come il nucleo (`salva_indirizzi`): la scoperta in IPv6 non
 * esiste, e un indirizzo che qui passa e di là no sarebbe un errore generico
 * dopo il clic. Ogni ottetto da 0 a 255 e senza zeri davanti — `010` è dieci
 * per qualcuno e otto per altri, e chi lo scrive non sa a quale dei due sta
 * parlando. La porta da 1 a 65535; senza porta vale [`PORTA`], che è quella di
 * chi ospita quasi sempre. `null` quando non si legge.
 */
export function leggiIndirizzo(testo: string): string | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?::(\d{1,5}))?$/.exec(testo.trim());
  if (m === null) return null;
  const ottetti = m.slice(1, 5) as [string, string, string, string];
  for (const o of ottetti) {
    if (o.length > 1 && o.startsWith("0")) return null;
    if (Number(o) > 255) return null;
  }
  const porta = m[5] === undefined ? PORTA : Number(m[5]);
  if (m[5] !== undefined && m[5].length > 1 && m[5].startsWith("0")) return null;
  if (!Number.isInteger(porta) || porta < 1 || porta > 65535) return null;
  return `${ottetti.join(".")}:${porta}`;
}

/** Com'è andata l'aggiunta di un indirizzo all'elenco. */
export type Aggiunta =
  | { elenco: string[]; errore: null }
  | { elenco: null; errore: Chiave };

/**
 * Aggiunge un indirizzo all'elenco, o dice perché no.
 *
 * Tre rifiuti, ciascuno con la sua frase: non si legge, c'è già, l'elenco è
 * pieno. Il doppione si riconosce **dopo** averlo scritto nella forma del
 * nucleo — `10.0.0.5` e `10.0.0.5:47811` sono lo stesso indirizzo, e due righe
 * uguali farebbero due domande alla stessa porta a ogni giro di scoperta.
 */
export function aggiungiIndirizzo(elenco: readonly string[], testo: string): Aggiunta {
  const letto = leggiIndirizzo(testo);
  if (letto === null) return { elenco: null, errore: "together.addresses.invalid" };
  if (elenco.includes(letto)) return { elenco: null, errore: "together.addresses.duplicate" };
  if (elenco.length >= MASSIMO_INDIRIZZI) {
    return { elenco: null, errore: "together.addresses.full" };
  }
  return { elenco: [...elenco, letto], errore: null };
}
