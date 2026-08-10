/**
 * Il manifest di un tema che non esiste ancora.
 *
 * # Perché non lo fa il nucleo
 *
 * Perché non c'è niente da decidere: un tema nuovo è una skin che c'è già con
 * `id` e `meta` riscritti, e riscriverli è una modifica sul testo come tutte
 * quelle dello Studio. Un comando `studio_crea` avrebbe dovuto portarsi dietro
 * un secondo posto in cui è scritto com'è fatto un manifest di partenza — e
 * quel posto esiste già, si chiama `plain.json`.
 *
 * # L'identificatore
 *
 * È il vincolo stretto del formato (`document.rs`, `identificatore`): da 2 a 48
 * fra minuscole, cifre e trattini, e comincia con una lettera. Le due guardie di
 * percorso in `skin.rs` e `studio.rs` sono più larghe — accettano maiuscole e
 * `_` — ma sono guardie, non la regola: un id che passa loro e non passa il
 * formato produce un errore alla validazione, cioè dopo, quando il tema è già
 * stato battezzato.
 */
import { leggi, scriviIn, togliDa } from "./patch";

/** Da 2 a 48: la finestra dice l'id mentre si scrive il nome, e deve dirlo giusto. */
const MIN = 2;
const MAX = 48;

/**
 * L'identificatore che viene da un nome scritto a mano.
 *
 * `presi` sono gli id già in uso: la collisione non è un errore da mostrare —
 * due temi si possono chiamare uguale — ma due file non si possono chiamare
 * uguale, quindi il secondo diventa `-2`.
 */
export function idDa(nome: string, presi: readonly string[]): string {
  const radice = accorcia(sillabe(nome));
  if (!presi.includes(radice)) return radice;
  // Da 2 in poi, e senza tetto: chi arriva al ventesimo «Nuovo tema» merita
  // comunque un id suo.
  for (let n = 2; ; n += 1) {
    const coda = `-${n}`;
    const candidato = `${accorcia(radice, MAX - coda.length)}${coda}`;
    if (!presi.includes(candidato)) return candidato;
  }
}

/** Il nome ridotto all'alfabeto dell'id, senza ancora guardare la lunghezza. */
function sillabe(nome: string): string {
  const spogliato = nome
    // `NFD` scompone «ù» in `u` più un segno combinante, e il segno si **toglie**
    // invece di cadere fra i caratteri non ammessi: lasciarlo diventare un
    // trattino darebbe `nottu-rno` invece di `notturno`.
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

  // Un nome scritto in un alfabeto che l'id non ammette — «夜曲», un'emoji — non
  // lascia niente da cui partire.
  if (spogliato.length === 0) return "tema";
  // Deve cominciare con una lettera ed essere lungo almeno due: «2001» e «A»
  // sono nomi legittimi che il formato rifiuta. In tutti e due i casi si
  // appoggia a `tema-`, che tiene il nome visibile invece di sostituirlo.
  return /^[a-z]/.test(spogliato) && spogliato.length >= MIN
    ? spogliato
    : `tema-${spogliato}`;
}

/** Tagliato alla lunghezza, senza lasciare un trattino appeso in fondo. */
function accorcia(id: string, max: number = MAX): string {
  return id.length <= max ? id : id.slice(0, max).replace(/-+$/, "");
}

/** Quel che la finestrella raccoglie. */
export interface DatiTema {
  /** L'identificatore, già derivato e già libero. */
  id: string;
  nome: string;
  autore: string;
  /** Vuota vuol dire assente: `meta.description` è facoltativa. */
  descrizione: string;
  /** L'id della skin da cui si parte, che finisce in `meta.basedOn`. */
  base: string;
}

/**
 * Il manifest del tema nuovo, o `null` se la base non si legge.
 *
 * # Perché può fallire
 *
 * Perché la base può essere una bozza lasciata a metà di una parentesi:
 * `studio_documento` preferisce le bozze alle skin installate, ed è giusto che
 * lo faccia. Su un testo che non è JSON `scriviIn` restituisce il testo
 * **intatto** — è la sua promessa, e serve a non riscrivere sopra il lavoro di
 * chi sta scrivendo. Qui però un testo intatto vorrebbe dire un manifest che
 * porta ancora l'id della base, e installarlo sovrascriverebbe la skin da cui si
 * voleva partire. Quindi si controlla prima, e si ricontrolla dopo.
 */
export function sorgenteNuova(base: string, dati: DatiTema): string | null {
  if (leggi(base) === null) return null;

  let fuori = base;
  fuori = scriviIn(fuori, ["id"], dati.id);
  fuori = scriviIn(fuori, ["meta", "name"], dati.nome);
  fuori = scriviIn(fuori, ["meta", "author"], dati.autore);
  // Un tema nuovo è alla sua prima versione, qualunque cosa dicesse la base.
  fuori = scriviIn(fuori, ["meta", "version"], "1.0.0");
  fuori =
    dati.descrizione.length > 0
      ? scriviIn(fuori, ["meta", "description"], dati.descrizione)
      : togliDa(fuori, ["meta", "description"]);
  fuori = scriviIn(fuori, ["meta", "basedOn"], dati.base);

  // La rete: se una sola delle scritture non è passata, l'id è ancora quello
  // della base, e da qui in giù nessuno se ne accorgerebbe più.
  return leggi(fuori)?.["id"] === dati.id ? fuori : null;
}
