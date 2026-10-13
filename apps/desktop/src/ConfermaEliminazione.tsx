/**
 * «Sono tre brani, e uno lo stai ascoltando: sicuro?»
 *
 * Una finestrella sua e non un componente condiviso, per la stessa ragione per
 * cui `schermate/Cartelle.tsx` tiene la sua: la domanda è di questo gesto —
 * togliere dei brani dal menù di una riga — e la apre soltanto `App`. Stava in
 * fondo ad `App.tsx`; adesso ha un file come le altre conferme
 * (`ConfermaRimozioni`, `ConfermaTrasloco`, `ConfermaSkin`), e non è cambiata.
 * Il comportamento — il fuoco che entra, la trappola del Tab, Escape, il
 * fuoco che torna a chi l'aveva, la pagina dietro inerte — viene tutto da
 * `Dialogo`, che la apre come `alertdialog`.
 *
 * # Due testi e non due finestrelle
 *
 * Perché la struttura è la stessa e a cambiare è solo quel che si sta per
 * perdere. Tenerle separate vorrebbe dire due volte la stessa impalcatura, e
 * due occasioni di correggerne una sola.
 *
 * Il tasto che conferma **non** è primario. Il primario è «Annulla»: in una
 * finestra che si apre sopra un gesto distruttivo, l'azione che il colore invita
 * a premere deve essere quella che non toglie niente.
 *
 * # Dove un Cestino non c'è
 *
 * Prima di mostrare il tasto, per «Elimina dal disco» si chiede al nucleo
 * quanti file stanno su un'unità di rete (`brani_su_rete`): lì il Cestino non
 * esiste, e Windows cancellerebbe per sempre dicendo di averli messi nel
 * Cestino. Se ce n'è anche uno solo la finestrella lo dice con il numero, e il
 * tasto dice «per sempre»: è quella conferma — e solo quella — che il nucleo
 * accetta per cancellarli. Finché la risposta non c'è il tasto resta spento:
 * confermare senza averla letta vorrebbe dire confermare un'altra frase. Se la
 * domanda fallisce si conta tutto come di rete, che fa chiedere di più e non
 * di meno.
 */
import { useEffect, useState } from "react";

import { Dialogo } from "./Dialogo";
import { brani_ } from "./formato";
import { ipc } from "./ipc";
import { t } from "./lingue";

export function ConfermaEliminazione({
  brani,
  dalDisco,
  onConferma,
  onChiudi,
}: {
  brani: number[];
  dalDisco: boolean;
  onConferma: (definitivi: boolean) => void;
  onChiudi: () => void;
}) {
  const quanti = brani.length;
  const [suRete, setSuRete] = useState<number | null>(dalDisco ? null : 0);
  useEffect(() => {
    if (!dalDisco) return;
    let viva = true;
    ipc
      .braniSuRete(brani)
      .then((n) => {
        if (viva) setSuRete(n);
      })
      .catch(() => {
        if (viva) setSuRete(brani.length);
      });
    return () => {
      viva = false;
    };
  }, [brani, dalDisco]);
  const perSempre = dalDisco && suRete !== null && suRete > 0;
  const tuttiInRete = perSempre && (suRete ?? 0) >= quanti;
  const titolo = dalDisco
    ? t("delete.disk.title", { brani: brani_(quanti) })
    : t("delete.library.title", { brani: brani_(quanti) });
  return (
    <Dialogo
      titolo={titolo}
      // Tutti su un'unità di rete: del Cestino non si parla, e nemmeno di «gli
      // altri», che non ci sono.
      descrizione={
        !dalDisco
          ? t("delete.library.body")
          : tuttiInRete
            ? t("delete.disk.bodyNetwork")
            : t("delete.disk.body")
      }
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
    >
      {perSempre && (
        <p className="nota">
          {t(tuttiInRete ? "delete.disk.networkAll" : "delete.disk.network", {
            n: suRete ?? 0,
            brani: brani_(suRete ?? 0),
          })}
        </p>
      )}
      <div className="tasti-finestrella">
        <button type="button" className="bottone primario" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="bottone"
          disabled={suRete === null}
          aria-busy={suRete === null}
          onClick={() => onConferma(perSempre)}
        >
          {!dalDisco
            ? t("delete.library.ok")
            : perSempre
              ? t("delete.disk.okForever")
              : t("delete.disk.ok")}
        </button>
      </div>
    </Dialogo>
  );
}
