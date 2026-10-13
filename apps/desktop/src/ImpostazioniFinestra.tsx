/**
 * Le Impostazioni della finestra: quelle vere, collegate a quel che mostrano.
 *
 * # Perché non sono più settanta prop scritte in `App`
 *
 * `Impostazioni` riceve per prop tutto quel che disegna, ed è giusto che resti
 * così: lo Studio la disegna con valori finti, passati a mano
 * (`studio/mondoImpostazioni.ts`), e una schermata che leggesse da sé gli
 * archivi della finestra non si potrebbe disegnare per finta. Ma le settanta
 * prop stavano scritte dentro `App`, quasi tutte come frecce nuove a ogni
 * disegno: qualunque cambio di `App` — una riga scelta, una lettera nella
 * ricerca, un passo del volume — rifaceva da capo un componente di tremila
 * righe che non era nemmeno cambiato.
 *
 * Qui le prop si compongono vicino a dove vengono. Quel che non è di `App` si
 * legge da sé: la libreria (`avvio`, la scansione, il suo esito), lo stato del
 * motore (soltanto le sei voci che la schermata disegna, scelte una per una),
 * le importazioni, il backup e la sincronia. Quel che è di `App` — la sezione,
 * l'aspetto, la skin e i gesti che toccano finestrelle e pagine — arriva in
 * quattro oggetti che restano gli stessi finché non cambia quel che portano.
 * E il componente è sotto `memo`: si ridisegna quando cambia qualcosa che
 * mostra, non quando cambia la finestra.
 *
 * # Le sezioni
 *
 * Dentro `Impostazioni` si disegna una sezione alla volta, e quelle che
 * leggono dal nucleo per conto loro — i testi, la dissolvenza, lo
 * spegnimento, la manutenzione — sono già componenti con lo stato loro. Il
 * resto si ridisegna con la schermata, che adesso si ridisegna di rado.
 */
import { memo } from "react";

import { useArchivio } from "./archivio";
import type { Aspetto } from "./aspettoFinestra";
import { ipc, type Avanzamento, type StatoRiproduzione } from "./ipc";
import { useLibreria } from "./libreria";
import { useNuvolaDellaFinestra } from "./nuvola";
import { useImportazioniDellaFinestra } from "./parti/Importazioni";
import type { Vista } from "./parti/Navigazione";
import { useSpecchioPresente } from "./presenza";
import { useLettore } from "./riproduzione";
import { Impostazioni, type Sezione } from "./schermate/Impostazioni";
import type { SkinDellaFinestra } from "./skinAttiva";

/**
 * I gesti delle Impostazioni che toccano `App`: le sue finestrelle, le sue
 * pagine, le sue cartelle.
 *
 * Un oggetto solo, con un'identità sola: `App` lo costruisce con funzioni che
 * non cambiano (`useStabile`), e il `memo` di qui può fidarsene.
 */
export interface AzioniImpostazioni {
  /** Rifà il giro guidato. */
  onGiro: () => void;
  /** Un profilo è stato applicato: quel che sta in `App` va riletto. */
  onProfiloImportato: () => void;
  onAggiungiCartella: () => void;
  onTogliCartella: (percorso: string) => void;
  onScegliCartellaDownload: () => void;
  onScansiona: () => void;
  /** Apre la finestrella che battezza un tema nuovo. */
  onCreaTema: () => void;
  /** Come `onCreaTema`, con questa skin già scelta come base. */
  onDeriva: (id: string) => void;
  onApriStudio: (id: string) => void;
  onImporta: () => void;
  onImportaLink: () => void;
  onImportaAccount: () => void;
  onVista: (vista: Vista) => void;
  /** Apre la finestrella del ripristino. */
  onNuvolaRipristina: () => void;
}

/** Le sei voci del motore che la schermata disegna: vedi `useLettore`. */
function delLettore(s: StatoRiproduzione) {
  return {
    eqAttivo: s.eqAttivo,
    eqGuadagni: s.eqGuadagni,
    replaygain: s.replaygain,
    spegnimentoMs: s.spegnimentoMs,
    autoplay: s.autoplay,
    dissolvenzaS: s.dissolvenzaS,
  };
}

type DelLettore = ReturnType<typeof delLettore>;

/**
 * Le sei voci sono le stesse di prima.
 *
 * Una a una, e la curva dell'equalizzatore banda per banda: ogni
 * `riproduzione:stato` — una pausa, un brano nuovo — arriva con un elenco di
 * guadagni nuovo anche quando i numeri sono quelli di prima, e confrontarlo
 * per identità ridisegnerebbe la schermata a ogni pausa.
 */
function ugualiDelLettore(a: DelLettore, b: DelLettore): boolean {
  return (
    a.eqAttivo === b.eqAttivo &&
    a.replaygain === b.replaygain &&
    a.spegnimentoMs === b.spegnimentoMs &&
    a.autoplay === b.autoplay &&
    a.dissolvenzaS === b.dissolvenzaS &&
    a.eqGuadagni.length === b.eqGuadagni.length &&
    a.eqGuadagni.every((g, i) => Object.is(g, b.eqGuadagni[i]))
  );
}

/** La scansione così com'è: per `useSpecchioPresente`, che vuole una scelta. */
function intera(scansione: Avanzamento | null): Avanzamento | null {
  return scansione;
}

/** Vedi il commento in testa. */
export const ImpostazioniDellaFinestra = memo(function ImpostazioniDellaFinestra({
  sezione,
  onSezione,
  onNotizia,
  onErrore,
  aspetto,
  skin,
  azioni,
}: {
  sezione: Sezione;
  onSezione: (s: Sezione) => void;
  onNotizia: (testo: string) => void;
  onErrore: (e: unknown) => void;
  aspetto: Aspetto;
  skin: SkinDellaFinestra;
  azioni: AzioniImpostazioni;
}) {
  const libreria = useLibreria();
  const avvio = useArchivio(libreria.avvio);
  const esito = useArchivio(libreria.esito);
  // Dallo specchio, come la pila dei toast: vedi «La scansione e le
  // transizioni» in `libreria.ts`.
  const scansione = useSpecchioPresente(libreria.scansione, intera);
  // Sei voci e non lo stato intero: un passo del volume, una pausa, un brano
  // nuovo non le cambiano, e non ridisegnano la schermata.
  const lettore = useLettore(delLettore, ugualiDelLettore);
  const importazioni = useImportazioniDellaFinestra();
  const {
    nuvola,
    sincronia,
    setSincronia,
    arricchimento,
    setArricchimento,
    avanzaArricchimento,
    esitoArricchimento,
    conNuvola,
    conSincronia,
  } = useNuvolaDellaFinestra();
  const skinAttiva = skin.skinAttiva;

  return (
    <Impostazioni
      sezione={sezione}
      onSezione={onSezione}
      onNotizia={onNotizia}
      avvio={avvio}
      scansione={scansione}
      esito={esito}
      skin={skin.skin}
      ripiego={skinAttiva?.ripiego ?? null}
      accentoPermesso={skinAttiva?.dynamicAccent ?? false}
      accentoDinamico={aspetto.accentoDinamico}
      onAccentoDinamico={(attivo) => void aspetto.cambiaAccentoDinamico(attivo)}
      movimento={skinAttiva?.layout.motion ?? "full"}
      tema={aspetto.tema}
      onTema={aspetto.cambiaTema}
      movimentoUtente={aspetto.movimentoUtente}
      onMovimentoUtente={aspetto.cambiaMovimentoUtente}
      zoom={aspetto.zoom ?? 1}
      onZoom={aspetto.cambiaZoom}
      onZoomNormale={aspetto.zoomNormale}
      lingua={aspetto.lingua}
      onLingua={aspetto.cambiaLingua}
      scorciatoie={aspetto.scorciatoie}
      onScorciatoie={aspetto.cambiaScorciatoie}
      onGiro={azioni.onGiro}
      onProfiloImportato={azioni.onProfiloImportato}
      eqAttivo={lettore.eqAttivo}
      eqGuadagni={lettore.eqGuadagni}
      replaygain={lettore.replaygain}
      onReplaygain={(livello) => {
        // Nessun `setStato` qui: il comando manda `riproduzione:stato`, e
        // la linguetta si sposta quando il motore ha davvero cambiato
        // posizione. Anticiparlo mostrerebbe il livello nuovo anche se il
        // salvataggio fallisse.
        ipc.normalizzazione(livello).catch(onErrore);
      }}
      spegnimentoMs={lettore.spegnimentoMs}
      onSpegnimento={(minuti) => {
        ipc.spegnimento(minuti).catch(onErrore);
      }}
      autoplay={lettore.autoplay}
      onAutoplay={(attivo) => {
        ipc.autoplay(attivo).catch(onErrore);
      }}
      dissolvenzaS={lettore.dissolvenzaS}
      onDissolvenza={(secondi) => {
        ipc.dissolvenza(secondi).catch(onErrore);
      }}
      onErrore={onErrore}
      onAggiungiCartella={azioni.onAggiungiCartella}
      onTogliCartella={azioni.onTogliCartella}
      onScegliCartellaDownload={azioni.onScegliCartellaDownload}
      onCartellaDownloadDiSerie={() => {
        // Stringa vuota: il nucleo toglie la riga e torna alla prima
        // cartella sorvegliata.
        ipc
          .impostaCartellaDownload("")
          .then(libreria.ricarica)
          .catch(onErrore);
      }}
      onScansiona={azioni.onScansiona}
      onAnnullaScansione={() => {
        // Non si tocca `scansione`: la barra resta finché il nucleo non
        // risponde. Nascondere l'avanzamento qui direbbe «fermata» prima
        // che lo sia — la scansione si ferma alla fine del lotto in corso,
        // e quel lotto può durare ancora qualche secondo.
        ipc.annullaScansione().catch(onErrore);
      }}
      onScegliSkin={(id) => void skin.scegliSkin(id)}
      onAnteprimaSkin={skin.anteprimaSkin}
      onInstallaSkin={() => void skin.installaSkin()}
      onCreaTema={azioni.onCreaTema}
      onDeriva={azioni.onDeriva}
      onDisinstallaSkin={(id) => void skin.disinstallaSkin(id)}
      onApriStudio={azioni.onApriStudio}
      onImporta={azioni.onImporta}
      onImportaLink={azioni.onImportaLink}
      onImportaAccount={azioni.onImportaAccount}
      onVista={azioni.onVista}
      importazioni={importazioni}
      arricchimento={arricchimento}
      avanzaArricchimento={avanzaArricchimento}
      esitoArricchimento={esitoArricchimento}
      onArricchimentoAttiva={(attivo) => {
        ipc
          .arricchimentoAttiva(attivo)
          .then(setArricchimento)
          .catch(onErrore);
      }}
      onArricchimentoAnnulla={() => {
        // Ottimistico su `inCorso`: riscrivere i tag di centinaia di file
        // sono decine di secondi, e senza questo il pulsante resterebbe
        // premibile per tutto quel tempo — con il risultato che chi non
        // vede succedere niente clicca due volte e si prende un
        // `metadata.enrichBusy`.
        setArricchimento((prima) =>
          prima ? { ...prima, inCorso: true } : prima,
        );
        ipc
          .arricchimentoAnnulla()
          .then((esito) => setArricchimento(esito.stato))
          .catch(onErrore);
      }}
      onArricchimentoRiportaNeiFile={() => {
        // Ottimistico come sopra, e qui serve ancora di più: questo riapre
        // in scrittura un file per ogni riga di `enrich_undo`, e su una
        // libreria vera sono decine di secondi in cui non si vede
        // succedere niente.
        setArricchimento((prima) =>
          prima ? { ...prima, inCorso: true } : prima,
        );
        ipc
          .arricchimentoRiportaNeiFile()
          .then((esito) => setArricchimento(esito.stato))
          .catch(onErrore);
      }}
      nuvola={nuvola}
      onNuvolaCollega={() => conNuvola(() => ipc.nuvolaCollega())}
      onNuvolaScollega={() => conNuvola(() => ipc.nuvolaScollega())}
      onNuvolaAttiva={(attivo) => conNuvola(() => ipc.nuvolaAttiva(attivo))}
      // Torna subito: quel che succede dopo arriva sull'evento.
      onNuvolaSalva={() => {
        ipc.nuvolaSalva().catch(onErrore);
      }}
      onNuvolaRipristina={azioni.onNuvolaRipristina}
      onNuvolaCredenziali={(id, segreto) =>
        conNuvola(() => ipc.nuvolaCredenziali(id, segreto))
      }
      sincronia={sincronia}
      onSincroniaAttiva={(accesa) => conSincronia(() => ipc.sincroniaAttiva(accesa))}
      onSincroniaMagazzino={(dove, cartella) =>
        conSincronia(() => ipc.sincroniaMagazzino(dove, cartella))
      }
      // Aspetta davvero: una passata a vuoto è un'elencazione, e su una
      // cartella condivisa non tocca nemmeno la rete. Quel che torna lo
      // rimanda comunque l'evento, quindi qui basta non perdere l'errore.
      onSincroniaAdesso={() => {
        setSincronia((prima) => (prima ? { ...prima, inCorso: true } : prima));
        ipc
          .sincroniaAdesso()
          .catch(onErrore)
          .finally(() => {
            ipc.sincroniaStato().then(setSincronia).catch(onErrore);
          });
      }}
      onSincroniaAccoppia={(id, nome) => {
        ipc
          .sincroniaAccoppia(id, nome)
          .then((dispositivi) =>
            setSincronia((prima) => (prima ? { ...prima, dispositivi } : prima)),
          )
          .catch(onErrore);
      }}
      onSincroniaDimentica={(id) => {
        ipc
          .sincroniaDimentica(id)
          .then((dispositivi) =>
            setSincronia((prima) => (prima ? { ...prima, dispositivi } : prima)),
          )
          .catch(onErrore);
      }}
    />
  );
});
