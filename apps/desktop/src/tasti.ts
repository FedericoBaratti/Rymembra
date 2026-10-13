/**
 * Come si chiama un tasto premuto: dal carattere, e se non basta dalla posizione.
 *
 * # Il carattere prima di tutto
 *
 * Le associazioni di `tastiera.ts` sono scritte col **carattere** che il tasto
 * produce (`e.key`), non con la sua posizione sulla tastiera (`e.code`), ed è la
 * scelta giusta: «Ctrl+F» vuol dire «la F», dovunque la F stia. Su una tastiera
 * tedesca la Z sta dove l'americana ha la Y, e chi preme `Ctrl+Z` vuole la Z.
 *
 * # E poi la posizione
 *
 * Il carattere tradisce chi scrive con un alfabeto che non è quello latino. Su
 * una tastiera russa lo stesso tasto della F produce «а», e `Ctrl+F` arriva come
 * `Ctrl+а`: nessuna associazione lo conosce, e la ricerca non si apre. In
 * francese le cifre stanno sotto Maiusc, e `Ctrl+0` arriva come `Ctrl+à`. Per
 * quei casi — e **solo** per quelli — si guarda la posizione: `KeyF` è «la F»
 * anche quando il carattere è cirillico.
 *
 * «Solo per quelli» vuol dire due condizioni insieme: il carattere arrivato non
 * è ASCII (quindi la tastiera non è latina, o il tasto è una lettera accentata),
 * e la posizione è una lettera o una cifra. Con la sola seconda, su una tastiera
 * italiana il tasto dell'apostrofo — che sta dove l'americana ha il trattino,
 * `Minus` — farebbe `Ctrl+-` e rimpicciolirebbe la finestra a chi voleva
 * scrivere un apostrofo. Il carattere ASCII si prende sempre per quel che è.
 *
 * # Perché un modulo a parte
 *
 * Perché è puro, e si prova: `tastiera.ts` legge le lingue, che fuori da Vite
 * non si caricano.
 */

/** I tasti che da soli non sono un tasto. */
const SOLO_MODIFICATORI = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

/** Quel che di un `KeyboardEvent` serve a dargli un nome. */
export interface Pressione {
  key: string;
  /** La posizione fisica. Facoltativo: chi simula un tasto a volte non lo scrive. */
  code?: string | undefined;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/** I modificatori, nell'ordine in cui si scrivono, con in coda il tasto. */
function componi(e: Pressione, tasto: string): string {
  const parti: string[] = [];
  if (e.ctrlKey) parti.push("Ctrl");
  if (e.shiftKey) parti.push("Shift");
  if (e.altKey) parti.push("Alt");
  parti.push(tasto);
  return parti.join("+");
}

/**
 * Il nome stabile di un tasto premuto, modificatori compresi.
 *
 * `Meta` non entra mai: il tasto Windows appartiene al sistema operativo, e una
 * scorciatoia che ci sopra non la vedremmo comunque arrivare per intera.
 * Restituisce `null` quando è stato premuto soltanto un modificatore — cioè
 * mentre si sta ancora componendo la combinazione, che è esattamente l'istante
 * in cui la cattura non deve concludere niente.
 */
export function tastoDi(e: Pressione): string | null {
  if (e.metaKey || SOLO_MODIFICATORI.has(e.key)) return null;
  // Una lettera si normalizza in minuscolo: con `Shift` premuto `e.key` è
  // maiuscola, e `Shift+F` e `Shift+f` sarebbero due associazioni diverse per
  // la stessa pressione.
  return componi(
    e,
    e.key === " " ? "Space" : e.key.length === 1 ? e.key.toLowerCase() : e.key,
  );
}

/**
 * La lettera o la cifra che sta in quella **posizione** su una tastiera
 * americana, o `null`: `KeyF` → `f`, `Digit0` → `0`.
 *
 * Le sole lettere e cifre, e non la punteggiatura: vedi «E poi la posizione».
 */
export function carattereDellaPosizione(code: string | undefined): string | null {
  if (code === undefined) return null;
  const lettera = /^Key([A-Z])$/.exec(code);
  if (lettera?.[1] !== undefined) return lettera[1].toLowerCase();
  const cifra = /^Digit([0-9])$/.exec(code);
  if (cifra?.[1] !== undefined) return cifra[1];
  return null;
}

/**
 * Il nome del tasto letto dalla posizione, quando il carattere non basta.
 *
 * `null` quando il carattere è ASCII — si prende per quel che è — o quando la
 * posizione non è una lettera né una cifra. Vedi la nota del modulo.
 */
export function tastoDallaPosizione(e: Pressione): string | null {
  if (e.metaKey || SOLO_MODIFICATORI.has(e.key)) return null;
  if (e.key.length !== 1 || /^[\x20-\x7e]$/.test(e.key)) return null;
  const carattere = carattereDellaPosizione(e.code);
  return carattere === null ? null : componi(e, carattere);
}

/**
 * I nomi con cui cercare questa pressione fra le associazioni, in ordine.
 *
 * Prima il carattere, poi — se c'è — la posizione. Chi cerca prende il primo
 * che trova: una tastiera latina non arriva mai al secondo.
 */
export function nomiDi(e: Pressione): string[] {
  const dalCarattere = tastoDi(e);
  if (dalCarattere === null) return [];
  const dallaPosizione = tastoDallaPosizione(e);
  return dallaPosizione === null || dallaPosizione === dalCarattere
    ? [dalCarattere]
    : [dalCarattere, dallaPosizione];
}

/**
 * Questa pressione è la lettera `lettera` (minuscola), su qualunque tastiera.
 *
 * Per i tasti fissi che non passano dalla tabella delle associazioni, come il
 * `Ctrl+A` delle liste: senza, su una tastiera russa «seleziona tutto» sarebbe
 * `Ctrl+ф`.
 */
export function eLaLettera(e: Pressione, lettera: string): boolean {
  if (e.key.toLowerCase() === lettera) return true;
  if (e.key.length === 1 && /^[\x20-\x7e]$/.test(e.key)) return false;
  return carattereDellaPosizione(e.code) === lettera;
}
