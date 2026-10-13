/**
 * Le tre misure di una copertina, e come chiederle al motore di rendering.
 *
 * # Perché tre
 *
 * Il nucleo ne serve tre dallo stesso protocollo (`src-tauri/src/copertine.rs`):
 * la miniatura da 160 pixel di lato, la media da 320 e la piena da 640. Le
 * griglie chiedevano la miniatura, e una scheda della griglia è larga dai 164
 * pixel in su: a 100% la miniatura era già un filo più piccola della scheda, a
 * 200% — uno schermo ad alta densità, o lo zoom dell'interfaccia — era un JPEG
 * da 160 ingrandito più del doppio. La media esiste per questo caso: nitida a
 * 100%, e la piena per chi ha il doppio dei pixel.
 *
 * # Perché `srcset` e non una scelta fatta qui
 *
 * Perché la densità vera la sa il motore e non la finestra: lo zoom della
 * WebView, quello di Windows e lo schermo su cui la finestra è stata trascinata
 * cambiano `devicePixelRatio` senza che nessuno ridisegni. Con `srcset` e
 * `sizes` si dice cosa c'è e quanto è larga la scheda, e la scelta la fa chi
 * conosce i pixel — e la rifà da sé quando cambiano.
 *
 * La funzione [`misuraPer`] fa la stessa scelta qui, per i test e per chi volesse
 * sapere in anticipo quale misura verrà chiesta: non è quella che decide.
 */
import { urlCopertina } from "./aspetto";

/** Le misure che il protocollo serve. */
export type MisuraCopertina = "miniatura" | "media" | "piena";

/**
 * Il lato di ciascuna misura, in pixel.
 *
 * Sono i `MAX_LATO_*` di `core/aether-app/src/covers.rs`: un lato è un massimo,
 * e una copertina più piccola dell'originale resta della sua misura. `srcset`
 * dichiara il massimo, che è quel che serve a scegliere.
 */
export const LATO: Readonly<Record<MisuraCopertina, number>> = {
  miniatura: 160,
  media: 320,
  piena: 640,
};

/** Le misure dalla più piccola alla più grande: l'ordine in cui si sceglie. */
const IN_ORDINE: readonly MisuraCopertina[] = ["miniatura", "media", "piena"];

/**
 * L'indirizzo di una misura.
 *
 * Dalla stessa `urlCopertina` che usa tutto il resto, con il suffisso del
 * protocollo per la media: un secondo modo di comporre l'indirizzo sarebbe un
 * secondo posto da cambiare il giorno in cui cambia lo schema.
 */
export function indirizzoCopertina(hash: string, misura: MisuraCopertina): string {
  switch (misura) {
    case "miniatura":
      return urlCopertina(hash, true) ?? "";
    case "piena":
      return urlCopertina(hash, false) ?? "";
    case "media":
      return `${urlCopertina(hash, false) ?? ""}.m`;
  }
}

/** Le tre misure in forma di `srcset`, con la larghezza di ciascuna. */
export function srcsetCopertina(hash: string): string {
  return IN_ORDINE.map((m) => `${indirizzoCopertina(hash, m)} ${LATO[m]}w`).join(", ");
}

/**
 * `sizes` per una copertina larga `larghezzaCss` pixel.
 *
 * Arrotondato per eccesso: mezzo pixel in meno può far scegliere la misura
 * sotto a una densità che sta proprio sul bordo. Senza una misura — la griglia
 * non si è ancora disegnata — si dice la colonna più stretta che il foglio
 * prevede: il motore sceglierà al più una misura sotto, per il tempo di un
 * disegno.
 */
export function sizesPer(larghezzaCss: number | null | undefined): string {
  const l =
    larghezzaCss !== null && larghezzaCss !== undefined && larghezzaCss > 0
      ? Math.ceil(larghezzaCss)
      : 164;
  return `${l}px`;
}

/**
 * La misura che un motore sceglierebbe da [`srcsetCopertina`].
 *
 * La più piccola che copre i pixel veri — larghezza per densità — e la piena se
 * nessuna li copre. È la regola dei motori: scegliere sotto vuol dire
 * ingrandire, scegliere sopra solo scaricare di più.
 */
export function misuraPer(larghezzaCss: number, densita: number): MisuraCopertina {
  const servono = larghezzaCss * (densita > 0 ? densita : 1);
  return IN_ORDINE.find((m) => LATO[m] >= servono) ?? "piena";
}
