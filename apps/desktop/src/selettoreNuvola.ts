/**
 * Il selettore delle cartelle del cloud, senza il disegno: dove si è, cosa si
 * è già chiesto, e cosa si mostra.
 *
 * # Perché a parte
 *
 * `AggiungiCartella.tsx` è una finestrella con sei linguette, tre moduli e
 * un'attesa del browser: la parte che si sbaglia senza vederlo non è lì. È qui,
 * ed è di due specie.
 *
 * **La risposta che arriva tardi.** Si entra in «Musica», l'elenco parte; si
 * entra subito in «Rock» prima che torni. Se la risposta di «Musica» si
 * scrivesse sopra «quel che si sta guardando», sotto le briciole «Musica ›
 * Rock» comparirebbero le sottocartelle di «Musica». Qui le risposte non
 * scrivono mai la vista: scrivono un archivio, chiave per chiave, e la vista è
 * la lettura della chiave di adesso. Una risposta in ritardo riempie il posto
 * suo, e tornandoci sopra è già pronta.
 *
 * **Tornare indietro.** È il gesto più frequente di un selettore — si scende
 * per guardare, si risale per scegliere altro — e se ogni risalita chiedesse
 * di nuovo l'elenco al servizio, ogni risalita sarebbe un secondo di
 * rotellina. L'archivio dura quanto il dialogo: dentro una sessione di scelta,
 * le cartelle di un drive non cambiano abbastanza da valere un'attesa, e
 * riaprendo il dialogo si riparte da capo.
 *
 * Il resto — le preimpostazioni di WebDAV, quale conto mostrare — sta qui per
 * la stessa ragione: è logica, si prova da sola (`test/selettoreNuvola.test.ts`).
 */
import type { CartellaRemota, ContoNuvola, ServizioOfferto } from "./ipc";
import { schedaDi, type Scheda } from "./cartelleNuvola";

// ── dove si è ───────────────────────────────────────────────────────────────

/** Un gradino delle briciole: una cartella remota, o la cima (`id` nullo). */
export interface Passo {
  id: string | null;
  nome: string;
  /** Una cartella che il servizio non ha: vedi `CartellaRemota.virtuale`. */
  virtuale?: true;
}

/**
 * Le briciole di un conto, dalla cima alla cartella in cui si è.
 *
 * Mai vuote: il primo passo è la cima del drive, col nome del servizio.
 */
export type Briciole = readonly Passo[];

/** Le briciole di chi non si è ancora mosso: la cima, e basta. */
export function inCima(nomeCima: string): Briciole {
  return [{ id: null, nome: nomeCima }];
}

/** Si entra in una sottocartella. */
export function entra(briciole: Briciole, cartella: CartellaRemota): Briciole {
  const passo: Passo = cartella.virtuale
    ? { id: cartella.id, nome: cartella.nome, virtuale: true }
    : { id: cartella.id, nome: cartella.nome };
  return [...briciole, passo];
}

/** Si sale di un gradino. Dalla cima non si sale: si resta lì. */
export function risali(briciole: Briciole): Briciole {
  return briciole.length > 1 ? briciole.slice(0, -1) : briciole;
}

/** Si torna a un gradino delle briciole: quelli dopo spariscono. */
export function tornaA(briciole: Briciole, indice: number): Briciole {
  if (indice < 0 || indice >= briciole.length) return briciole;
  return briciole.slice(0, indice + 1);
}

/** La cartella in cui si è: il suo id, o `null` per la cima. */
export function dentro(briciole: Briciole): string | null {
  return briciole[briciole.length - 1]?.id ?? null;
}

/** Il nome della cartella in cui si è. */
export function nomeDiQui(briciole: Briciole): string {
  return briciole[briciole.length - 1]?.nome ?? "";
}

/**
 * I nomi dalla cima in giù, **senza** la cima: quel che `nuvolaAggiungi`
 * chiede per il messaggio d'attesa («Musica», «Rock»). La cima è il nome del
 * servizio, e il nucleo il servizio lo sa già.
 */
export function nomiDalla(briciole: Briciole): string[] {
  return briciole.slice(1).map((passo) => passo.nome);
}

/** Si è dentro una cartella che il servizio non ha? */
export function inVirtuale(briciole: Briciole): boolean {
  return briciole[briciole.length - 1]?.virtuale === true;
}

/**
 * Si può usare la cartella in cui si è?
 *
 * Non la cima: `nuvolaAggiungi` vuole l'id di una cartella, e la cima un id
 * non ce l'ha (`nuvolaCartelle` la chiama `null`). È anche il caso in cui una
 * sola parola sbagliata costerebbe di più — un drive intero con le sue
 * fotografie, letto per cercarci dei brani.
 *
 * E non una cartella virtuale: raccoglie cartelle di altri, e un elenco suo da
 * seguire non ce l'ha.
 */
export function usabile(briciole: Briciole): boolean {
  return dentro(briciole) !== null && !inVirtuale(briciole);
}

/**
 * L'id da cui si è appena risaliti, per rimettergli il fuoco.
 *
 * Chi sale con Backspace vuole ritrovarsi sulla cartella da cui è uscito, non
 * in cima all'elenco: è quel che fa ogni gestore di file, ed è quel che rende
 * «scendo a guardare e torno» un gesto solo.
 */
export function daDoveSiRisale(prima: Briciole, dopo: Briciole): string | null {
  if (dopo.length >= prima.length) return null;
  return prima[dopo.length]?.id ?? null;
}

// ── cosa si è chiesto ───────────────────────────────────────────────────────

/** La chiave di un elenco: il conto e la cartella. */
export function chiaveElenco(conto: string, cartella: string | null): string {
  // Un carattere che né un conto né un id possono contenere: `conto` è
  // `servizio-impronta`, e gli id dei servizi sono stampabili.
  return `${conto}\u0000${cartella ?? ""}`;
}

/** Quel che il dialogo ha chiesto finora, e com'è andata. */
export interface Elenchi {
  /** Le risposte arrivate, per chiave. */
  arrivati: ReadonlyMap<string, readonly CartellaRemota[]>;
  /** I guasti, per chiave: restano finché non si riprova. */
  guasti: ReadonlyMap<string, unknown>;
  /** Le domande partite e non ancora tornate. */
  inVolo: ReadonlySet<string>;
}

/** Niente chiesto. */
export const NESSUN_ELENCO: Elenchi = {
  arrivati: new Map(),
  guasti: new Map(),
  inVolo: new Set(),
};

/**
 * Va chiesto?
 *
 * No se c'è già, se è in volo, o se è fallito: un guasto non si richiede da
 * solo — lo si dice, e «Riprova» lo toglie. Ritentare in automatico vorrebbe
 * dire un servizio che non risponde martellato a ogni disegno.
 */
export function daChiedere(elenchi: Elenchi, chiave: string): boolean {
  return (
    !elenchi.arrivati.has(chiave) && !elenchi.inVolo.has(chiave) && !elenchi.guasti.has(chiave)
  );
}

/** La domanda è partita. */
export function chiesto(elenchi: Elenchi, chiave: string): Elenchi {
  const inVolo = new Set(elenchi.inVolo);
  inVolo.add(chiave);
  return { ...elenchi, inVolo };
}

/** La risposta è arrivata: si scrive al suo posto, qualunque cosa si guardi adesso. */
export function arrivato(
  elenchi: Elenchi,
  chiave: string,
  cartelle: readonly CartellaRemota[],
): Elenchi {
  const arrivati = new Map(elenchi.arrivati);
  arrivati.set(chiave, cartelle);
  const inVolo = new Set(elenchi.inVolo);
  inVolo.delete(chiave);
  const guasti = new Map(elenchi.guasti);
  guasti.delete(chiave);
  return { arrivati, guasti, inVolo };
}

/** La domanda è fallita. */
export function fallito(elenchi: Elenchi, chiave: string, guasto: unknown): Elenchi {
  const guasti = new Map(elenchi.guasti);
  guasti.set(chiave, guasto);
  const inVolo = new Set(elenchi.inVolo);
  inVolo.delete(chiave);
  return { ...elenchi, guasti, inVolo };
}

/** «Riprova»: il guasto si dimentica, e la chiave torna da chiedere. */
export function riprova(elenchi: Elenchi, chiave: string): Elenchi {
  if (!elenchi.guasti.has(chiave)) return elenchi;
  const guasti = new Map(elenchi.guasti);
  guasti.delete(chiave);
  return { ...elenchi, guasti };
}

/** Quel che il selettore mostra per una chiave. */
export type Vista =
  | { tipo: "attesa" }
  | { tipo: "guasto"; guasto: unknown }
  | { tipo: "vuota" }
  | { tipo: "elenco"; cartelle: readonly CartellaRemota[] };

/**
 * Quel che si mostra adesso.
 *
 * Una chiave mai chiesta è «attesa» anche lei: l'effetto che la chiede parte
 * dopo il disegno, e per quel disegno la verità è che la risposta non c'è.
 */
export function vista(elenchi: Elenchi, chiave: string): Vista {
  const cartelle = elenchi.arrivati.get(chiave);
  if (cartelle !== undefined) {
    return cartelle.length === 0 ? { tipo: "vuota" } : { tipo: "elenco", cartelle };
  }
  if (elenchi.guasti.has(chiave)) return { tipo: "guasto", guasto: elenchi.guasti.get(chiave) };
  return { tipo: "attesa" };
}

// ── le linguette ────────────────────────────────────────────────────────────

/** Una linguetta del selettore: un servizio, e per MEGA anche il suo link. */
export interface Linguetta {
  scheda: Scheda;
  /** Il servizio che dà il nome e il modo alla linguetta. */
  offerto: ServizioOfferto;
  /** Solo per MEGA: il servizio dei link di cartella, se la build lo offre. */
  link: ServizioOfferto | null;
}

/**
 * Le linguette, nell'ordine in cui il nucleo offre i servizi.
 *
 * Il link di MEGA non ne ha una sua (vedi `Scheda`): si appoggia a quella di
 * MEGA, e se la build offrisse soltanto lui la linguetta sarebbe sua.
 */
export function linguetteDi(servizi: readonly ServizioOfferto[]): Linguetta[] {
  const fuori: { scheda: Scheda; offerto: ServizioOfferto; link: ServizioOfferto | null }[] =
    [];
  for (const offerto of servizi) {
    const scheda = schedaDi(offerto.servizio);
    const gia = fuori.find((l) => l.scheda === scheda);
    if (offerto.servizio === "megalink") {
      if (gia === undefined) fuori.push({ scheda, offerto, link: offerto });
      else gia.link = offerto;
    } else if (gia === undefined) {
      fuori.push({ scheda, offerto, link: null });
    } else {
      gia.offerto = offerto;
    }
  }
  return fuori;
}

/**
 * Si può usare, arriverà, o chiede prima la sua app.
 *
 * `"daConfigurare"` non spegne la linguetta: fino alla 3.1 voleva dire «questa
 * build non porta l'identificativo dell'app», e la linguetta era spenta con la
 * sua ragione, perché chi guardava non poteva farci niente. Dalla 3.2 l'app è
 * dell'utente, e quel che manca lo si fa lì: la linguetta si apre e mostra il
 * modulo che la configura (`parti/AppServizio.tsx`), poi «Collega».
 */
export type StatoOfferta = "pronto" | "inArrivo" | "daConfigurare";

/**
 * Lo stato di un servizio, così come la linguetta lo deve dire.
 *
 * «In arrivo» vince su «da configurare»: far registrare un'app per un servizio
 * che comunque non funzionerebbe ancora sarebbe un lavoro per niente.
 */
export function statoOfferta(offerto: ServizioOfferto): StatoOfferta {
  if (!offerto.pronto) return "inArrivo";
  if (!offerto.disponibile) return "daConfigurare";
  return "pronto";
}

/** Lo stato di una linguetta: pronta se lo è almeno una delle sue due strade. */
export function statoLinguetta(linguetta: Linguetta): StatoOfferta {
  const strade = [linguetta.offerto, ...(linguetta.link === null ? [] : [linguetta.link])];
  const stati = strade.map(statoOfferta);
  if (stati.includes("pronto")) return "pronto";
  return stati.includes("daConfigurare") ? "daConfigurare" : "inArrivo";
}

/**
 * La linguetta si apre?
 *
 * Tutte tranne quelle «in arrivo»: una da configurare si apre sul suo modulo,
 * e sotto il modulo c'è la stessa strada di una pronta.
 */
export function apribile(linguetta: Linguetta): boolean {
  return statoLinguetta(linguetta) !== "inArrivo";
}

// ── quale conto ─────────────────────────────────────────────────────────────

/** I conti di una linguetta: MEGA prende anche i suoi link. */
export function contiDella(conti: readonly ContoNuvola[], scheda: Scheda): ContoNuvola[] {
  return conti.filter((c) => schedaDi(c.servizio) === scheda);
}

/** Un conto con cui si può sfogliare: il servizio lo accetta ancora. */
export function sfogliabile(conto: ContoNuvola): boolean {
  return conto.stato !== "daRicollegare";
}

/**
 * Quale conto mostrare aprendo una linguetta.
 *
 * Quello già scelto, se c'è ancora; altrimenti il primo che si può sfogliare;
 * altrimenti il primo e basta — un conto da ricollegare si mostra lo stesso,
 * perché è la sola cosa che spiega a chi guarda perché le sue cartelle non si
 * vedono.
 */
export function contoDaMostrare(
  conti: readonly ContoNuvola[],
  scelto: string | null,
): ContoNuvola | null {
  return (
    conti.find((c) => c.conto === scelto) ?? conti.find(sfogliabile) ?? conti[0] ?? null
  );
}

// ── WebDAV ──────────────────────────────────────────────────────────────────

/** Una preimpostazione di WebDAV: il nome del servizio e la forma del suo indirizzo. */
export interface Preimpostazione {
  chiave: string;
  nome: string;
  /**
   * L'indirizzo, coi pezzi da scrivere fra graffe: `{server}` è il nome del
   * server di chi lo usa, `{utente}` il suo nome utente. Restano visibili nel
   * campo apposta: un segnaposto da sostituire si vede, un indirizzo già
   * completo e sbagliato no.
   */
  forma: string;
}

/**
 * Le cinque che il nucleo conosce, nell'ordine in cui le si incontra più spesso.
 *
 * Nextcloud e ownCloud stanno sul server di chi li usa; pCloud, Koofr e kDrive
 * hanno un indirizzo fisso, con al più l'identificativo del drive. Per pCloud
 * la forma è quella europea (`ewebdav`): chi ha il conto negli Stati Uniti
 * toglie la «e», e la nota del modulo lo dice.
 */
export const PREIMPOSTAZIONI_WEBDAV: readonly Preimpostazione[] = [
  {
    chiave: "nextcloud",
    nome: "Nextcloud",
    forma: "https://{server}/remote.php/dav/files/{utente}/",
  },
  {
    chiave: "owncloud",
    nome: "ownCloud",
    forma: "https://{server}/remote.php/dav/files/{utente}/",
  },
  { chiave: "pcloud", nome: "pCloud", forma: "https://ewebdav.pcloud.com/" },
  { chiave: "koofr", nome: "Koofr", forma: "https://app.koofr.net/dav/Koofr/" },
  {
    chiave: "kdrive",
    nome: "kDrive",
    forma: "https://{server}.connect.kdrive.infomaniak.com/",
  },
];

/**
 * L'indirizzo da mandare: `{utente}` diventa l'utente scritto nel modulo.
 *
 * Nextcloud e ownCloud vogliono il nome utente due volte — nel percorso e nel
 * campo — e chiedere di scriverlo due volte è chiedere di sbagliarlo una.
 * `{server}` invece resta: quello lo sa solo chi scrive.
 */
export function indirizzoDaMandare(indirizzo: string, utente: string): string {
  const nome = utente.trim();
  const pulito = indirizzo.trim();
  return nome.length === 0
    ? pulito
    : pulito.replaceAll("{utente}", encodeURIComponent(nome));
}

/** Cosa non va in un indirizzo WebDAV, prima di mandarlo. */
export type DifettoIndirizzo =
  | "vuoto"
  | "segnaposto"
  | "credenziali"
  | "http"
  | "nonHttps";

/**
 * Il difetto di un indirizzo, o `null` se si può mandare.
 *
 * Detto **prima** di provare, per i casi che il nucleo rifiuterebbe comunque
 * dopo un giro in rete: un segnaposto rimasto fra graffe, utente e password
 * dentro l'indirizzo (`https://utente:password@server/…`, che terrebbe la
 * password in chiaro col conto: `errors.cloud.credentialsInAddress`),
 * `http://` (la password viaggerebbe in chiaro: `errors.cloud.insecureServer`),
 * e qualcosa che un indirizzo non è.
 *
 * Le credenziali si cercano **solo nell'autorità**, fra «://» e la prima «/»,
 * come fa `scomponi` nel nucleo (`rymembra-cloud`, `musica/webdav.rs`): una
 * chiocciola nel percorso (`…/dav/files/mario@esempio.it/`) è un nome utente
 * fatto da un'email, e va bene. Vengono prima di `http`: sono il difetto da
 * togliere comunque, anche passando a `https://`.
 */
export function difettoIndirizzo(indirizzo: string): DifettoIndirizzo | null {
  const pulito = indirizzo.trim();
  if (pulito.length === 0) return "vuoto";
  if (/[{}]/.test(pulito)) return "segnaposto";
  const schema = pulito.indexOf("://");
  if (schema >= 0) {
    const resto = pulito.slice(schema + "://".length);
    const fine = resto.indexOf("/");
    const autorita = fine >= 0 ? resto.slice(0, fine) : resto;
    if (autorita.includes("@")) return "credenziali";
  }
  const minuscolo = pulito.toLowerCase();
  if (minuscolo.startsWith("http://")) return "http";
  if (!minuscolo.startsWith("https://") || pulito.length <= "https://".length) {
    return "nonHttps";
  }
  return null;
}

// ── MEGA ────────────────────────────────────────────────────────────────────

/**
 * Il link di una cartella MEGA ha la forma giusta?
 *
 * Solo la forma, e solo per dirlo subito: la chiave dopo il `#` è la parte che
 * si perde copiando male — molte app di chat la tagliano — e senza quella il
 * nucleo risponderebbe `cloud.linkInvalid` dopo un giro inutile. Se la forma
 * va, decide il nucleo.
 */
export function linkMegaPlausibile(link: string): boolean {
  const pulito = link.trim();
  return /^https:\/\/(www\.)?mega\.(nz|io)\/(folder\/[^#\s]+#[^\s]+|#F![^!\s]+![^\s]+)$/i.test(
    pulito,
  );
}

/** Il codice della verifica in due passaggi: sei cifre, spazi tolti. */
export function codicePulito(codice: string): string {
  return codice.replace(/\s+/g, "");
}

/** Il codice ha sei cifre? */
export function codiceCompleto(codice: string): boolean {
  return /^\d{6}$/.test(codicePulito(codice));
}
