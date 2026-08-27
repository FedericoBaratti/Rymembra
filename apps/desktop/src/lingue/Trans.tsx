/**
 * Un testo con dentro dei nodi.
 *
 * # Perché non basta `t()`
 *
 * Il grosso del testo di Aether non sono etichette: sono capoversi che spiegano
 * qualcosa, e dentro hanno markup — un `<code>` che nomina un token, uno
 * `<strong>` che porta un valore calcolato. Per esempio, dalle impostazioni:
 *
 * ```tsx
 * Il movimento lo dichiara la skin (<code>motion.intensity</code>), e questa ne
 * dice <strong>{movimento}</strong>.
 * ```
 *
 * Spezzarlo in tre chiavi da concatenare produce traduzioni impossibili: in
 * un'altra lingua l'ordine delle parti cambia, e chi traduce riceve tre
 * frammenti senza sapere come si rimonteranno. La chiave dev'essere **una**, con
 * i segnaposto dentro, così chi traduce vede la frase intera e li sposta dove la
 * sua grammatica li vuole.
 *
 * # Come
 *
 * Gli stessi segnaposto `{nome}` di `t()`, ma rimpiazzati con nodi React invece
 * che con testo. Un segnaposto che non ha un nodo resta scritto com'è — è
 * visibile, ed è il comportamento giusto per un difetto di traduzione: si nota.
 */
import { Fragment, type ReactNode } from "react";

import { type Chiave, type Valori, t } from ".";

/** I nodi da infilare, per nome del segnaposto. */
export type Nodi = Record<string, ReactNode>;

/**
 * Il testo della chiave, coi segnaposto sostituiti da nodi.
 *
 * `v` porta i nodi, `n` gli eventuali valori testuali — un `{conteggio}` che è
 * solo un numero non merita un nodo, e passarlo da `n` lo lascia dentro la
 * stringa dove la traduzione lo può spostare senza che React lo veda mai.
 */
export function Trans({
  k,
  v,
  n,
}: {
  k: Chiave;
  v?: Nodi;
  n?: Valori;
}): ReactNode {
  const testo = t(k, n);
  if (v === undefined) return testo;

  // La cattura tiene i segnaposto dentro il risultato dello split, così i pezzi
  // pari sono testo e i dispari sono nomi. Un solo passaggio, nessuna ricerca
  // all'indietro.
  const pezzi = testo.split(/\{(\w+)\}/g);
  return (
    <>
      {pezzi.map((pezzo, indice) => {
        if (indice % 2 === 0)
          return pezzo === "" ? null : (
            <Fragment key={indice}>{pezzo}</Fragment>
          );
        const nodo = v[pezzo];
        return (
          <Fragment key={indice}>
            {nodo === undefined ? `{${pezzo}}` : nodo}
          </Fragment>
        );
      })}
    </>
  );
}
