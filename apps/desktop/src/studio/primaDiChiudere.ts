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
 *
 * Le strade che non passano dalla pagina — Alt+F4, la barra delle
 * applicazioni, «Esci» dal vassoio — chiudono dal nucleo. Per quelle il
 * nucleo deve sapere che c'è qualcosa da aspettare: `quandoCambiaDaScrivere`
 * glielo dice (la barra del titolo lo inoltra), e il nucleo rinvia la
 * chiusura, chiede alla pagina di scrivere e chiude quando lei risponde — o
 * dopo un tetto suo. Vedi `src-tauri/src/chiusura.rs`.
 */

const attese = new Set<() => Promise<void>>();

/** Chi vuole sapere se c'è qualcosa da scrivere prima di chiudere. */
const ascolti = new Set<(si: boolean) => void>();

function avvisa(si: boolean): void {
  // Una copia: chi ascolta può smettere proprio adesso.
  for (const fai of [...ascolti]) fai(si);
}

/** Registra `fai` fino alla chiamata della funzione che restituisce. */
export function primaDiChiudere(fai: () => Promise<void>): () => void {
  const prima = attese.size;
  attese.add(fai);
  if (prima === 0 && attese.size === 1) avvisa(true);
  return () => {
    if (attese.delete(fai) && attese.size === 0) avvisa(false);
  };
}

/**
 * Ascolta quando comincia e quando finisce l'«ho qualcosa da scrivere»: chi si
 * iscrive sente subito com'è adesso. Restituisce chi smette.
 */
export function quandoCambiaDaScrivere(fai: (si: boolean) => void): () => void {
  ascolti.add(fai);
  fai(attese.size > 0);
  return () => {
    ascolti.delete(fai);
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
