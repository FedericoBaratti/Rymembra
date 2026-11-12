/**
 * Quel che va scritto prima che la X della finestra chiuda Rymembra.
 *
 * Il salvataggio dello Studio parte 800 ms dopo l'ultima modifica, e la X
 * chiude davvero (a meno di «tieni in secondo piano», spento di serie): un
 * nome scritto e subito dopo la X, e riaprendo il nome non c'era (R14-7).
 * Lo Studio si registra qui finché è aperto, e la X della barra del titolo
 * (`BarraTitolo.tsx`) aspetta che abbia scritto prima di chiedere la chiusura.
 *
 * Con un tetto: un disco che non risponde non deve tenere aperta una finestra
 * che si è chiesto di chiudere, e un guasto si dice già da chi salva.
 * Restano fuori le strade che non passano dalla pagina — Alt+F4, la barra
 * delle applicazioni, «Esci» dal vassoio — che chiudono dal nucleo.
 */

const attese = new Set<() => Promise<void>>();

/** Registra `fai` fino alla chiamata della funzione che restituisce. */
export function primaDiChiudere(fai: () => Promise<void>): () => void {
  attese.add(fai);
  return () => {
    attese.delete(fai);
  };
}

/** Aspetta chi è registrato, al più `tetto` millisecondi. Non fallisce mai. */
export async function tuttoScritto(tetto = 3000): Promise<void> {
  if (attese.size === 0) return;
  let conto: ReturnType<typeof setTimeout> | undefined;
  await Promise.race([
    Promise.allSettled([...attese].map((fai) => fai())),
    new Promise<void>((basta) => {
      conto = setTimeout(basta, tetto);
    }),
  ]);
  if (conto !== undefined) clearTimeout(conto);
}
