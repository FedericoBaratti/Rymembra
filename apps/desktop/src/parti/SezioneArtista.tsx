/**
 * Una sezione della pagina di un artista, sotto la griglia dei dischi: «Brani»,
 * «Compare anche in», «Forse compare anche in».
 *
 * Una regione con il suo titolo (`h2`): chi ascolta lo schermo salta da una
 * sezione all'altra per regione, e ogni elenco di righe qui sotto ha un nome
 * invece di tre elenchi anonimi uno dopo l'altro. Il conto sta accanto al
 * titolo, e una `nota` dice per cosa sono le righe — per le partecipazioni
 * euristiche, che il nome è stato cercato e non letto, e che può sbagliare.
 */
import type { ReactNode } from "react";

export function SezioneArtista({
  id,
  titolo,
  conto,
  nota,
  azioni,
  children,
}: {
  /** Un `id` per il titolo, che nomina la regione. */
  id: string;
  titolo: string;
  /** Quanti: «12 brani». */
  conto?: string | undefined;
  /** Una riga sotto il titolo, per quel che il titolo non dice. */
  nota?: string | undefined;
  /** Quel che sta a destra del titolo: l'ordinamento. */
  azioni?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="sezione-artista" aria-labelledby={id}>
      <header className="testa-sezione-artista">
        <h2 id={id} className="titolo-sezione-artista" dir="auto">
          {titolo}
        </h2>
        {conto !== undefined && <span className="conto-sezione-artista">{conto}</span>}
        {azioni !== undefined && <div className="azioni-sezione-artista">{azioni}</div>}
      </header>
      {nota !== undefined && <p className="nota-sezione-artista">{nota}</p>}
      {children}
    </section>
  );
}
