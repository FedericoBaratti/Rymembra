/**
 * La pagina «Dispositivi» (`src/parti/Dispositivi.tsx`): quel che il nucleo non
 * vede e la finestra deve fare da sé. «Smetti di aspettare» mentre il
 * collegamento si sta accendendo (reperto R13-2, decisione D-26), e la conferma
 * prima di «Dimentica» per tutti e tre i tipi di dispositivo (D-28).
 *
 * Senza un DOM nelle prove: la sequenza «accendi poi collega» è una funzione
 * pura di `lontano.ts`, e il resto si guarda nel sorgente del componente.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { StatoLontano } from "../src/ipc";
import { accendiPoiCollega, archivioLontano, tentativiDiCollegamento } from "../src/lontano";

const DISPOSITIVI = readFileSync(
  new URL("../src/parti/Dispositivi.tsx", import.meta.url),
  "utf8",
).replace(/\r\n/g, "\n");

/**
 * Il testo da `apertura` alla chiusura che lo segue: `}` a capo per una
 * funzione, `  };` per una costante dentro un componente.
 */
function funzione(sorgente: string, apertura: string, chiusura = "\n}\n"): string {
  const inizio = sorgente.indexOf(apertura);
  assert.ok(inizio >= 0, `non trovo «${apertura}»`);
  const fine = sorgente.indexOf(chiusura, inizio);
  assert.ok(fine > inizio, `non trovo la fine di «${apertura}»`);
  return sorgente.slice(inizio, fine);
}

function stato(): StatoLontano {
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    errore: null,
    dispositivi: [],
    invito: null,
    richieste: [],
  };
}

test("«Smetti di aspettare» mentre il collegamento si accende: non ci si collega", async () => {
  // Reperto R13-2: la finestrella diceva già «Aspetto la conferma…», chi la
  // lasciava annullava un collegamento che non c'era ancora, e appena
  // `lontanoAttiva` rispondeva il collegamento partiva lo stesso — senza più
  // niente da premere per fermarlo.
  const tentativi = tentativiDiCollegamento();
  const annullato = tentativi.nuovo();
  let arriva: (pannello: StatoLontano) => void = () => undefined;
  let collegati = 0;
  const esito = accendiPoiCollega({
    attiva: () => new Promise<StatoLontano>((fatto) => (arriva = fatto)),
    collega: () => collegati++,
    annullato,
  });
  tentativi.lascia();
  arriva(stato());
  assert.equal(await esito, false);
  assert.equal(collegati, 0, "lasciato mentre si accendeva: nessun collegamento");
  assert.equal(archivioLontano.leggi()?.attivo, true, "il pannello acceso arriva comunque");

  // Senza lasciare, ci si collega.
  const altro = tentativi.nuovo();
  assert.equal(
    await accendiPoiCollega({ attiva: async () => stato(), collega: () => collegati++, annullato: altro }),
    true,
  );
  assert.equal(collegati, 1);
  // Un tentativo vecchio resta annullato anche dopo che ne comincia uno nuovo.
  assert.equal(annullato(), true);
  assert.equal(altro(), false);
  archivioLontano.scrivi(null);
});

test("la finestrella del link accende e collega con la sequenza che sa di essere stata lasciata", () => {
  const accendi = funzione(DISPOSITIVI, "const accendiECollega =", "\n  };\n");
  assert.match(accendi, /accendiPoiCollega\(/);
  assert.doesNotMatch(accendi, /\.then\(\(nuovo\) => \{[^}]*collega\(/, "niente collegamento in un `then` cieco");
  const lascia = funzione(DISPOSITIVI, "const lascia =", "\n  };\n");
  assert.match(lascia, /tentativi\.lascia\(\)/, "lasciare annulla anche il tentativo che si sta accendendo");
});

test("«Dimentica» chiede conferma per tutti e tre i tipi di dispositivo", () => {
  // Decisione D-28: chiedeva conferma solo per i collegati via internet; un
  // PC di «Ascolta insieme» o un dispositivo della sincronia si dimenticavano
  // al primo clic.
  for (const riga of ["function RigaFidato(", "function RigaSincronia(", "function RigaDispositivo("]) {
    const corpo = funzione(DISPOSITIVI, riga);
    const tasto = corpo.slice(corpo.lastIndexOf("<button"), corpo.indexOf('t("devices.forget")}'));
    assert.match(tasto, /onDomanda\(\{/, `${riga} chiede prima di dimenticare`);
    assert.match(tasto, /consenso: true/, `${riga}: è un consenso, che le skin non ridisegnano`);
  }
});
