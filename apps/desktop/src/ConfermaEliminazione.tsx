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
 * quali dei file scelti stanno dove un Cestino non c'è (`brani_su_rete`, che
 * a dispetto del nome conta anche le unità rimovibili — chiavette, schede SD
 * —, i CD e i RAM disk): lì Windows cancellerebbe per sempre dicendo di averli
 * messi nel Cestino. Se ce n'è anche uno solo la finestrella lo dice con il
 * numero, li elenca, e il tasto dice «per sempre». Finché la risposta non c'è
 * il tasto resta spento: confermare senza averla letta vorrebbe dire
 * confermare un'altra frase.
 *
 * # La conferma è l'elenco
 *
 * Non un sì: l'elenco dei file mostrati, che torna al nucleo così com'è. Era un
 * sì, e all'esecuzione il nucleo richiede dove sta ogni file: una share lenta a
 * rispondere poteva lasciare senza risposta — cioè senza Cestino — un file
 * locale che qui era stato promesso al Cestino, e il sì dato per gli altri
 * valeva anche per lui. Adesso un file senza Cestino che non sta nell'elenco fa
 * rifiutare tutto prima di toccare qualunque cosa, e `App` riapre questa
 * finestrella con `cambiata`: la domanda si rifà, e si risponde a quel che è
 * vero adesso.
 *
 * Per la stessa ragione, se la domanda fallisce la finestrella dice perché e il
 * tasto non c'è: un elenco che non è arrivato non si può confermare, e
 * inventarne uno vorrebbe dire far cancellare per sempre file mai mostrati.
 *
 * Il numero è di brani, non di file, perché si confronta con `quanti` per
 * decidere se dire «gli altri vanno nel Cestino»: i dodici brani di
 * un'immagine CD sono due file — l'immagine e il `.cue` — e contati come file
 * facevano dire «2 brani su rete» mentre ne sparivano per sempre dodici. E
 * conta solo i brani scelti, quindi non supera mai `quanti`: la vecchia riga
 * del disco intero, se c'è ancora, se ne va con l'immagine anche senza averla
 * scelta, ma non fa dire «tutti» a una scelta che ha anche un file locale.
 *
 * Tranne un caso: i brani di un'immagine CD scelti solo in parte
 * (`library.imageTrackPartialDelete`). Lì il file è di tutto il disco, e il
 * nucleo rifiuterebbe comunque: la finestrella dice perché e non offre il
 * tasto, invece di chiedere un sì a un gesto che non succederà.
 *
 * # Fuori dalle skin
 *
 * È un dialogo di consenso (`consenso`, cioè `data-consenso` sulla radice): la
 * frase che dice «per sempre» e l'elenco dei file non si lasciano ridisegnare
 * da una skin.
 */
import { useEffect, useState } from "react";

import { Dialogo } from "./Dialogo";
import { brani_ } from "./formato";
import { ipc, testoErrore, type SenzaCestino } from "./ipc";
import { t } from "./lingue";

/** Quel che si sa prima di chiedere: niente per sempre, niente da elencare. */
const NESSUNO: SenzaCestino = { brani: 0, file: [] };

export function ConfermaEliminazione({
  brani,
  dalDisco,
  cambiata = false,
  onConferma,
  onChiudi,
}: {
  brani: number[];
  dalDisco: boolean;
  /**
   * La si riapre perché il nucleo ha rifiutato la conferma di prima: un file
   * che non vi stava è risultato senza Cestino. Lo dice, prima del resto.
   */
  cambiata?: boolean | undefined;
  /** `definitivi`: i file mostrati come da cancellare per sempre, tali e quali. */
  onConferma: (definitivi: string[]) => void;
  onChiudi: () => void;
}) {
  const quanti = brani.length;
  const [senza, setSenza] = useState<SenzaCestino | null>(dalDisco ? null : NESSUNO);
  const [rifiuto, setRifiuto] = useState<string | null>(null);
  useEffect(() => {
    if (!dalDisco) return;
    // Una domanda nuova cancella la risposta di prima: confermare un elenco
    // vecchio vorrebbe dire rispondere a un'altra domanda.
    setSenza(null);
    setRifiuto(null);
    let viva = true;
    ipc
      .braniSuRete(brani)
      .then((risposta) => {
        if (viva) setSenza(risposta);
      })
      .catch((err: unknown) => {
        // Un'immagine CD scelta a metà, o una domanda che non ha avuto
        // risposta: in tutti e due i casi non c'è un elenco da confermare.
        if (viva) setRifiuto(testoErrore(err));
      });
    return () => {
      viva = false;
    };
  }, [brani, dalDisco]);
  const perSempre = dalDisco && senza !== null && senza.file.length > 0;
  const quantiPerSempre = senza?.brani ?? 0;
  const tuttiInRete = perSempre && quantiPerSempre >= quanti;
  // Un rifiuto non è una domanda: il brano di un'immagine CD scelto da solo
  // apriva «Eliminare 1 brano dal disco?» col testo sul Cestino, e sotto la
  // spiegazione che non si poteva, col solo «Annulla» (R18-16). Il titolo
  // dice il fatto, e il testo è il perché.
  const titolo =
    dalDisco && rifiuto !== null
      ? t("delete.disk.refused.title")
      : dalDisco
        ? t("delete.disk.title", { brani: brani_(quanti) })
        : t("delete.library.title", { brani: brani_(quanti) });
  return (
    <Dialogo
      titolo={titolo}
      // Tutti dove il Cestino non c'è (rete o unità rimovibile): del Cestino
      // non si parla, e nemmeno di «gli altri», che non ci sono.
      descrizione={
        !dalDisco
          ? t("delete.library.body")
          : rifiuto !== null
            ? rifiuto
            : tuttiInRete
              ? t("delete.disk.bodyNetwork")
              : t("delete.disk.body")
      }
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      consenso
      onChiudi={onChiudi}
    >
      {cambiata && <p className="nota">{t("delete.disk.changed")}</p>}
      {perSempre && senza !== null && (
        <>
          <p className="nota">
            {t(tuttiInRete ? "delete.disk.networkAll" : "delete.disk.network", {
              n: quantiPerSempre,
              brani: brani_(quantiPerSempre),
            })}
          </p>
          {/* L'elenco esatto di quel che non torna: è quel che si conferma.
              Aperto, perché «per sempre» si legge prima di premere; e i
              percorsi interi, perché due album dallo stesso nome su due
              dischi si distinguono solo così. */}
          <details className="spostamenti" open>
            <summary>{t("delete.disk.files", { n: senza.file.length })}</summary>
            <ul>
              {senza.file.map((percorso) => (
                <li key={percorso} className="percorso" title={percorso}>
                  {percorso}
                </li>
              ))}
            </ul>
          </details>
        </>
      )}
      {rifiuto !== null && !dalDisco && <p className="nota">{rifiuto}</p>}
      <div className="tasti-finestrella">
        <button type="button" className="bottone primario" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        {rifiuto === null && (
          <button
            type="button"
            className="bottone"
            disabled={senza === null}
            aria-busy={senza === null}
            onClick={() => onConferma(perSempre && senza !== null ? senza.file : [])}
          >
            {!dalDisco
              ? t("delete.library.ok")
              : perSempre
                ? t("delete.disk.okForever")
                : t("delete.disk.ok")}
          </button>
        )}
      </div>
    </Dialogo>
  );
}
