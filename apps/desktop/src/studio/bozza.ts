/**
 * La bozza dello Studio, per la finestra: l'id che il formato ammette, e se
 * «Butta la bozza» ha un posto a cui tornare.
 *
 * Due regole del nucleo che la finestra deve sapere **prima** di chiedere: il
 * campo dell'id scrive nel documento quando si esce, e il nucleo sposta la
 * bozza appena il documento dichiara un id nuovo; «Butta la bozza» cancella la
 * cartella e riapre il pacchetto. Il nucleo resta la guardia vera in tutti e
 * due i casi (`id_del_formato` e `scarta_bozza` in `studio.rs`); qui si evita
 * di offrire un gesto che finirebbe in un errore.
 *
 * Un modulo a parte e non dentro i componenti perché quelli portano React e
 * l'IPC, e `node --test` non li monta: le regole si provano in
 * `test/bozza.test.ts`.
 */

/**
 * L'id è uno che il formato accetta: da 2 a 48 fra minuscole, cifre e
 * trattini, e comincia con una lettera.
 *
 * È la regola di `identificatore` in `core/rymembra-skin/src/document.rs`,
 * ricopiata in `studio.rs` come `id_del_formato` (legata all'originale dalla
 * prova `l_id_dichiarato_segue_la_regola_del_formato`) e qui con la stessa
 * tabella di casi. Il campo accettava `/^[A-Za-z0-9_-]{1,64}$/`, la guardia
 * delle cartelle e non la regola: «Notturno» si scriveva nel documento, la
 * bozza si spostava sotto un id che la validazione poi rifiutava, e la
 * correzione in «notturno» trovava su Windows la propria cartella e la
 * scambiava per quella di un altro.
 *
 * Tutti i caratteri ammessi sono ASCII, quindi la lunghezza in unità UTF-16
 * di JavaScript è quella in byte che misura il nucleo.
 */
export function idDelFormato(id: string): boolean {
  return /^[a-z][a-z0-9-]{1,47}$/.test(id);
}

/**
 * La bozza `id` ha un pacchetto a cui tornare: una skin di serie, o una
 * installata — anche una che non passa più la validazione, che l'elenco porta
 * col nome del suo file — con lo stesso identificatore. `null` finché
 * l'elenco delle skin non c'è.
 *
 * È la domanda che `scarta_bozza` si fa prima di cancellare: senza un posto a
 * cui tornare la rifiuta (`skin.notFound`, «questa bozza non ha un pacchetto a
 * cui tornare»). Il caso normale è un id cambiato nel documento: Studio aperto
 * su Plain, id «mia», e la bozza sta in `bozze/mia` senza un `mia.ryskin`.
 */
export function haUnPacchetto(id: string, elenco: readonly { id: string }[] | null): boolean | null {
  return elenco === null ? null : elenco.some((voce) => voce.id === id);
}

/**
 * Che cosa può fare «Butta la bozza»: buttarla, niente perché la bozza è il
 * documento del pacchetto, o niente perché un pacchetto non c'è.
 */
export type Scarto = "si" | "niente" | "senzaPacchetto";

/**
 * Lo stato del tasto «Butta la bozza».
 *
 * «Senza pacchetto» vince su «niente da buttare»: quella frase dice che il
 * documento è quello del pacchetto, e qui un pacchetto non c'è. Finché
 * l'elenco delle skin non è arrivato — o non si è potuto leggere — vale la
 * regola di prima, cioè il solo testo: la guardia vera è nel nucleo, che
 * rifiuta senza toccare il disco, e un tasto spento per un elenco che non
 * arriva sarebbe un vicolo cieco.
 */
export function scarto(derivata: boolean, pacchetto: boolean | null): Scarto {
  if (pacchetto === false) return "senzaPacchetto";
  return derivata ? "si" : "niente";
}
