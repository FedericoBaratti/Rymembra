/**
 * «Questa cartella la sorvegli già, con un altro nome.»
 *
 * # Perché esiste
 *
 * Chi tiene la musica su un NAS la raggiunge in due modi: con una lettera
 * mappata (`Z:\`) e con il nome della condivisione (`\\nas\musica`). Per
 * Windows sono lo stesso posto; per una libreria che confronta stringhe erano
 * due cartelle, e aggiungere la seconda accanto alla prima voleva dire una
 * seconda scansione della stessa musica — ogni brano rientrato col nome nuovo,
 * senza ascolti, voti né playlist, accanto a quello vecchio.
 *
 * Il nucleo se ne accorge (`radici::ordina_cartelle`) e non la aggiunge: la
 * restituisce fra le «gemelle». Qui si chiede cosa farne.
 *
 * # Due uscite, e nessuna perde niente
 *
 * «Sposta» riscrive i percorsi dei brani sulla grafia nuova (`cartelle_trasloca`)
 * e tiene tutta la loro storia. «Lascia com'è» non fa niente: la cartella di
 * prima resta sorvegliata, e la nuova non entra. Il primario è il secondo,
 * perché è quello che non cambia niente — lo stesso criterio delle altre
 * finestrelle che si aprono sopra un gesto che riscrive.
 */
import { useState } from "react";
import { Dialogo } from "./Dialogo";
import type { CartellaGemella } from "./ipc";
import { t } from "./lingue";

export function ConfermaTrasloco({
  gemella,
  onSposta,
  onChiudi,
}: {
  gemella: CartellaGemella;
  onSposta: () => Promise<void>;
  onChiudi: () => void;
}) {
  // Il trasloco verifica ogni file sulla share prima di scrivere: su una
  // libreria grande sono secondi, e un secondo clic in mezzo ne farebbe
  // partire un altro sopra il primo.
  const [inCorso, setInCorso] = useState(false);
  const stessa = gemella.a === gemella.nuova;
  return (
    // Il velo resta spento mentre si sposta, com'era; Escape invece chiude
    // anche allora — il trasloco continua nel nucleo e la sua notizia arriva
    // nella fascia comunque.
    <Dialogo
      titolo={t("folders.twin.title")}
      descrizione={
        stessa
          ? t("folders.twin.same", {
              nuova: gemella.nuova,
              esistente: gemella.esistente,
            })
          : t("folders.twin.inside", {
              nuova: gemella.nuova,
              esistente: gemella.esistente,
            })
      }
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
      veloChiude={!inCorso}
      occupato={inCorso}
    >
      <p>{t("folders.twin.body", { a: gemella.a })}</p>
      <div className="tasti-finestrella">
        <button
          type="button"
          className="bottone primario"
          onClick={onChiudi}
          disabled={inCorso}
        >
          {t("folders.twin.keep")}
        </button>
        <button
          type="button"
          className="bottone"
          disabled={inCorso}
          onClick={() => {
            setInCorso(true);
            void onSposta().finally(() => setInCorso(false));
          }}
        >
          {inCorso ? t("folders.twin.moving") : t("folders.twin.move")}
        </button>
      </div>
    </Dialogo>
  );
}
