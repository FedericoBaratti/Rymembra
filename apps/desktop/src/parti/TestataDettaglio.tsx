/**
 * La testata di un disco, di un artista o di una playlist, sul telefono.
 *
 * Sul computer la pagina di un disco ha l'intestazione grande: la copertina,
 * il titolo, «Riproduci» (`Intestazione.tsx`). Sul telefono quella testata è
 * una barra (`BarraAlta`), e con lei se ne erano andati la copertina e il tasto
 * che fa partire il disco intero: si poteva solo toccare il primo brano. Qui
 * tornano, nella pagina e non nella barra, così scorrono via con lei e lasciano
 * all'elenco tutto lo schermo:
 *
 * - la copertina grande, o per un artista e una playlist un mosaico di quattro;
 * - che cos'è, il nome, e una riga di numeri;
 * - due tasti larghi: «Riproduci» dal primo, «Casuale» in un ordine qualunque.
 *
 * Dietro, la copertina sfocata che sfuma nel fondo della pagina: la stessa
 * ricetta di «In riproduzione», in piccolo.
 *
 * # Il titolo che passa nella barra
 *
 * Il nome qui è grande, e nella barra in cima no: la barra della pagina lo
 * mostra solo quando questo è uscito di sotto di lei, scorrendo. Lo fa il
 * foglio, con una timeline di scorrimento (`telefono-pagine.css`, «il titolo
 * che si ritira»): nessun ascoltatore di `scroll` qui, e nessun disegno di
 * React mentre il dito scorre. Dove la WebView non le conosce, la barra tiene
 * il titolo sempre: c'è due volte, che è meno grave di non esserci.
 */
import type { ReactNode } from "react";

import { Copertina, Sfocata } from "../Copertina";
import { t } from "../lingue";
import { Icona } from "./Icone";

export function TestataDettaglio({
  occhiello,
  titolo,
  sottotitolo,
  copertine,
  rotonda = false,
  spento,
  onRiproduci,
  onCasuale,
  sotto,
  conTitolo = false,
}: {
  occhiello: string;
  titolo: string;
  sottotitolo: string;
  /**
   * Le copertine: una per un disco, fino a quattro per un artista e una
   * playlist. Con quattro diverse si disegna il mosaico, con meno la prima.
   */
  copertine: readonly (string | null)[];
  /** Un artista: la copertina tonda, come la sua scheda. */
  rotonda?: boolean | undefined;
  /** Non c'è niente da suonare, o non c'è un'uscita audio. */
  spento: boolean;
  onRiproduci: () => void;
  onCasuale: () => void;
  /** Quel che sta sotto i tasti: la nota di una playlist intelligente. */
  sotto?: ReactNode;
  /**
   * Il nome è il titolo di questa parte della pagina, non lo stesso della
   * barra: in «Sfoglia» la barra dice il dispositivo, e il disco aperto dentro
   * la pagina ha il suo `h2`, che chi ascolta lo schermo deve sentire.
   */
  conTitolo?: boolean | undefined;
}) {
  const prima = copertine[0] ?? null;
  const mosaico = copertine.length >= 4 ? copertine.slice(0, 4) : null;
  return (
    <section className="testata-dettaglio" data-rotonda={rotonda || undefined}>
      <div className="sfondo-dettaglio" aria-hidden="true">
        <Sfocata hash={prima} classe="tinta-dettaglio" />
      </div>
      <div className="arte-dettaglio" aria-hidden="true">
        {mosaico !== null ? (
          <div className="mosaico-dettaglio">
            {mosaico.map((hash, i) => (
              <Copertina
                // La stessa copertina può tornare: la posizione la distingue.
                key={`${i}-${hash ?? ""}`}
                hash={hash}
                titolo={titolo}
                classe="tassello-dettaglio"
                misura="scheda"
                accanto
              />
            ))}
          </div>
        ) : (
          <Copertina hash={prima} titolo={titolo} classe="copertina-dettaglio" piena accanto />
        )}
      </div>
      <span className="occhiello-dettaglio">{occhiello}</span>
      {/* Di solito non è un titolo per chi ascolta lo schermo: il titolo della
          pagina è quello della barra (`h1`), che c'è sempre; questo è lo
          stesso nome, grande, per chi guarda. Ed è il bersaglio della
          timeline. Con `conTitolo` il nome è un altro, e allora è un `h2`. */}
      {conTitolo ? (
        <h2 className="titolo-dettaglio" dir="auto">
          {titolo}
        </h2>
      ) : (
        <p className="titolo-dettaglio" dir="auto" aria-hidden="true">
          {titolo}
        </p>
      )}
      <p className="sotto-dettaglio" dir="auto">
        {sottotitolo}
      </p>
      <div className="tasti-dettaglio">
        <button type="button" className="riproduci-dettaglio" disabled={spento} onClick={onRiproduci}>
          <Icona nome="i-play" dim={20} />
          {t("action.play")}
        </button>
        <button type="button" className="casuale-dettaglio" disabled={spento} onClick={onCasuale}>
          <Icona nome="i-shuffle" dim={20} />
          {t("action.shuffle")}
        </button>
      </div>
      {sotto}
    </section>
  );
}
