/**
 * L'artista intero: chi è l'artista di un brano per la sua pagina, come lo si
 * trova nella griglia, dove si torna chiudendola, e come si ordinano i suoi
 * brani.
 *
 * # Perché l'artista **effettivo**
 *
 * Gli elenchi di Rymembra mettono un brano sotto l'artista del disco quando
 * c'è, e sotto il suo interprete se manca (`ARTISTA_EFFETTIVO` in
 * `core/rymembra-app/src/library/aggregati.rs`). Il nome sul pulsante di «In
 * riproduzione» apre la pagina di *quello*: su un brano di una raccolta
 * l'interprete è «Tale» e la pagina che esiste è «Vari artisti». Per questo la
 * scheda del brano porta `albumArtist`, e la regola di SQL è scritta di nuovo
 * qui, una volta, con la sua prova.
 *
 * Senza React e senza il nucleo: le prove di `test/` lo caricano con
 * `node --test`.
 */
import { idDiLibreria } from "./insieme";
import type { Artista, Brano, OrdineArtista } from "./ipc/libreria";

/**
 * Toglie gli spazi attorno al nome come `TRIM` di SQLite: gli spazi e basta.
 * Un `trim()` di JavaScript toglierebbe anche tabulazioni e a capo, e il nome
 * che ne esce non sarebbe più quello della griglia degli artisti.
 */
function senzaSpazi(testo: string): string {
  return testo.replace(/^ +| +$/g, "");
}

/**
 * Il nome sotto cui la griglia degli artisti tiene questo brano: l'artista del
 * disco se il tag c'è, altrimenti l'interprete. Vuoto se non c'è né l'uno né
 * l'altro: il brano non ha una pagina da aprire.
 */
export function artistaEffettivo(brano: Pick<Brano, "artist" | "albumArtist">): string {
  const delDisco = senzaSpazi(brano.albumArtist ?? "");
  return delDisco !== "" ? delDisco : senzaSpazi(brano.artist);
}

/**
 * La pagina che il nome dell'artista di un brano apre, o `null` se non ne apre
 * nessuna e il nome resta testo.
 *
 * Solo una riga della libreria — anche quella di un altro dispositivo, che è una
 * riga specchio con un id positivo — ha una pagina da aprire. Una voce di «Ascolta
 * insieme» (id negativo) non è nella libreria: l'artista che la sessione dice è un
 * nome che questa libreria può non avere, e cercarlo per nome porterebbe a un
 * elenco vuoto o a un omonimo. Come cuore, stelle e menù (`idDiLibreria`), il
 * gesto c'è solo dove c'è una riga su cui farlo. E un brano senza artista non ha
 * una pagina.
 */
export function paginaDellArtista(
  brano: Pick<Brano, "id" | "artist" | "albumArtist">,
): string | null {
  if (!idDiLibreria(brano.id)) return null;
  const nome = artistaEffettivo(brano);
  return nome === "" ? null : nome;
}

/**
 * L'artista della griglia che ha questo nome: quello esatto, o il primo che gli
 * somiglia senza badare alle maiuscole. Il ripiego c'è per un nome scritto in
 * due modi nei tag: la griglia li tiene separati (la normalizzazione serve a
 * ordinarli, non a fonderli), ma chi apre la pagina da un brano vuole
 * arrivare da qualche parte.
 */
export function trovaArtista(artisti: readonly Artista[], nome: string): Artista | undefined {
  const piegato = nome.toLocaleLowerCase();
  return (
    artisti.find((a) => a.name === nome) ??
    artisti.find((a) => a.name.toLocaleLowerCase() === piegato)
  );
}

/**
 * Chiudere questa pagina dell'artista riapre «In riproduzione»?
 *
 * Sì se è stata aperta dal nome in «In riproduzione» — `daInRiproduzione` è il
 * nome dell'artista che ha aperto quel gesto — e non un'altra pagina nel
 * frattempo: Indietro torna da dove si è partiti, come ogni pagina di dettaglio.
 */
export function tornaAInRiproduzione(
  daInRiproduzione: string | null,
  aperto: Artista | null,
): boolean {
  return aperto !== null && daInRiproduzione !== null && aperto.name === daInRiproduzione;
}

/** I quattro ordinamenti della sezione «Brani», nell'ordine in cui si offrono. */
export const ORDINI_ARTISTA: readonly OrdineArtista[] = ["album", "anno", "titolo", "ascoltati"];

/**
 * Tutte le righe di un elenco a pagine, una pagina dopo l'altra.
 *
 * Per i gesti che vogliono **l'elenco intero** — «Riproduci», «Casuale» e
 * «Scarica qui» sull'artista — mentre la finestra ne tiene in mano qualche
 * pagina. Si avanza di quante righe sono arrivate, e ci si ferma sulla pagina
 * vuota, non su una più corta della misura chiesta: un nucleo che un giorno
 * limitasse la pagina farebbe suonare mezzo artista senza dirlo.
 *
 * `massimoPagine` è la rete contro una richiesta che ripete sempre le stesse
 * righe: superata, è un errore e non un elenco tagliato.
 *
 * @throws se le pagine non finiscono entro `massimoPagine`.
 */
export async function tutteLePagine<T>(
  chiedi: (offset: number, limite: number) => Promise<readonly T[]>,
  misura = 200,
  massimoPagine = 1_000,
): Promise<T[]> {
  const righe: T[] = [];
  for (let pagina = 0; pagina < massimoPagine; pagina += 1) {
    const arrivate = await chiedi(righe.length, misura);
    if (arrivate.length === 0) return righe;
    righe.push(...arrivate);
  }
  throw new Error(`l'elenco non finisce dopo ${massimoPagine} pagine`);
}
