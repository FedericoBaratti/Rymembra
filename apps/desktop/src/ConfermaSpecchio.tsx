/**
 * «Il PC non condivide più 1.340 brani: toglierli?»
 *
 * Con la libreria unica i brani di un altro dispositivo sono righe di questa
 * libreria, e quando il suo catalogo non li manda più il nucleo **non li toglie
 * subito**: dentro le playlist, un brano che sparisce sembrerebbe un brano
 * tolto a mano. Poche righe — una cartella cambiata, un album tolto — se ne
 * vanno da sole; oltre una soglia (cinquanta righe e un quinto del catalogo) il
 * nucleo aspetta un sì, come per le rimozioni di una scansione
 * (`ConfermaRimozioni.tsx`).
 *
 * La forma è la stessa, e per la stessa ragione: «Lasciali» è il primario, perché
 * è l'azione che non toglie niente. Il disco staccato, la cartella spostata, il PC
 * che ha riavviato a metà elenco sono il caso vero, e le righe tornano da sole
 * quando il dispositivo le rimanda. «Togli» le toglie dalla libreria — e, dove
 * non c'è un'altra copia, dalle playlist.
 *
 * `Dialogo` dà il fuoco che entra, la trappola del Tab, Escape (che vale
 * «Lasciali») e la pagina dietro inerte.
 */
import { useEffect, useState } from "react";

import { useArchivio } from "./archivio";
import { Dialogo } from "./Dialogo";
import { brani_ } from "./formato";
import { ipc } from "./ipc";
import { t } from "./lingue";
import {
  archivioLontano,
  archivioSpecchio,
  chiediLaFotografiaDelloSpecchio,
  domandaSullePerdite,
} from "./lontano";
import { unaVolta } from "./modale";
import { dispositiviDiLibreria } from "./origine";

export function ConfermaSpecchio({
  nome,
  quante,
  onTogli,
  onLascia,
}: {
  /** Il dispositivo. */
  nome: string;
  /** Quante righe non manda più. */
  quante: number;
  onTogli: () => void;
  onLascia: () => void;
}) {
  // Una risposta sola per tutte e due le uscite: un doppio clic su «Togli», o
  // «Togli» e poi Escape, non mandano due risposte (vedi `unaVolta`).
  const [prima] = useState(unaVolta);
  const lascia = () => {
    if (prima()) onLascia();
  };
  const togli = () => {
    if (prima()) onTogli();
  };
  return (
    <Dialogo
      titolo={t("devices.mirror.confirm.title", { nome, brani: brani_(quante) })}
      descrizione={t("devices.mirror.confirm.body")}
      ruolo="alertdialog"
      classe="finestrella media glass-modal"
      // Toglie righe dalla libreria: una skin non la ridisegna (D-51).
      consenso
      onChiudi={lascia}
    >
      <div className="tasti-finestrella">
        <button type="button" className="bottone primario" data-fuoco-iniziale onClick={lascia}>
          {t("devices.mirror.confirm.keep")}
        </button>
        <button type="button" className="bottone" onClick={togli}>
          {t("devices.mirror.confirm.remove")}
        </button>
      </div>
    </Dialogo>
  );
}

/**
 * La domanda, quando un dispositivo ha righe da confermare: una alla volta, e
 * una volta sola per numero.
 *
 * «Lasciali» si ricorda finché il numero non cambia: il nucleo rifà il conto a
 * ogni giro — ogni cinque minuti — e la stessa domanda alla stessa cifra
 * tornerebbe ogni cinque minuti. Un numero diverso è una situazione diversa, e si
 * chiede di nuovo. Il ricordo sta nel nucleo (`dispositivi::ricorda_lasciati`):
 * prima era solo qui, in memoria, e il riavvio lo scordava — la domanda tornava
 * alla stessa cifra. Finché il nucleo non ha risposto non si chiede niente
 * (`domandaSullePerdite`). Uno solo in tutto il programma, in `App`.
 */
export function DomandaSpecchio({
  onErrore,
  onNotizia,
  onRilegge,
}: {
  onErrore: (errore: unknown) => void;
  onNotizia: (testo: string) => void;
  /** La libreria è cambiata: rilegge gli elenchi. */
  onRilegge: () => void;
}) {
  const avanzamenti = useArchivio(archivioSpecchio);
  const stato = useArchivio(archivioLontano);
  // `null` è «il nucleo non ha ancora detto»: vedi `domandaSullePerdite`.
  const [lasciate, setLasciate] = useState<ReadonlyMap<string, number> | null>(null);
  useEffect(() => {
    let superata = false;
    ipc
      .lontanoSpecchioLasciati()
      .then((ricordate) => {
        // Quel che si è lasciato nel frattempo vince su quel che il nucleo aveva.
        if (!superata) {
          setLasciate((prima) => new Map([...Object.entries(ricordate), ...(prima ?? [])]));
        }
      })
      .catch((errore: unknown) => {
        // Si dice, e si va avanti: senza il ricordo la domanda torna, che è il
        // comportamento di prima e non toglie niente a nessuno.
        if (superata) return;
        onErrore(errore);
        setLasciate((prima) => prima ?? new Map());
      });
    return () => {
      superata = true;
    };
    // Una lettura sola, all'apertura: dopo, il ricordo lo scrive questo componente.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const nomi = new Map(dispositiviDiLibreria(stato).map((d) => [d.chiave, d.nome]));
  const domanda = domandaSullePerdite(avanzamenti, nomi, lasciate);
  if (domanda === undefined) return null;
  const nome = nomi.get(domanda.dispositivo) ?? "";
  const ricorda = () =>
    setLasciate((prima) => new Map(prima).set(domanda.dispositivo, domanda.inAttesa));
  return (
    <ConfermaSpecchio
      key={`${domanda.dispositivo}:${domanda.inAttesa}`}
      nome={nome}
      quante={domanda.inAttesa}
      onLascia={() => {
        ricorda();
        ipc.lontanoSpecchioLascia(domanda.dispositivo, domanda.inAttesa).catch(onErrore);
      }}
      onTogli={() => {
        // In memoria resta fino alla rilettura: il conto in attesa si azzera
        // quando il nucleo ha finito, e nel frattempo la domanda non torna. Nel
        // nucleo si dimentica: le righe non ci sono più, e se un giorno ne
        // aspetteranno altrettante è un'altra domanda.
        ricorda();
        ipc.lontanoSpecchioLascia(domanda.dispositivo, null).catch(onErrore);
        ipc
          .lontanoSpecchioConferma(domanda.dispositivo)
          .then((tolte) => {
            onNotizia(t("devices.mirror.removed", { nome, brani: brani_(tolte) }));
            onRilegge();
            // Adottata solo per i dispositivi su cui nel frattempo non è arrivato
            // un evento più nuovo (S9-13).
            const adotta = chiediLaFotografiaDelloSpecchio();
            return ipc.lontanoSpecchioStato().then(adotta);
          })
          .catch(onErrore);
      }}
    />
  );
}
