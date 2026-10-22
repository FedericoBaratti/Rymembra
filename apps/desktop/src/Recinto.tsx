/**
 * Il recinto: quel che resta in piedi quando l'interfaccia cade.
 *
 * # Il guasto che tiene chiuso
 *
 * Un errore lanciato durante il disegno di un componente React, se nessuno lo
 * prende, smonta l'albero intero. Quel che resta è una finestra bianca: nessun
 * messaggio, nessun tasto, niente da leggere e niente da premere. In sviluppo
 * c'è la console e ci si arriva in un minuto; in rilascio la console non c'è, e
 * di quel guasto non resta traccia da nessuna parte — né a schermo né nel
 * diario, che fino a ieri raccoglieva solo quel che succede nel nucleo.
 *
 * # Perché un componente di classe
 *
 * Perché `getDerivedStateFromError` e `componentDidCatch` esistono solo lì.
 * React non ha mai dato un gancio equivalente, e non è una svista: prendere un
 * errore di disegno vuol dire ricordarsi di averlo preso attraverso un
 * ri-disegno, che è esattamente quel che uno stato di classe fa e un `useState`
 * dentro il componente che sta cadendo non può fare.
 *
 * # Cosa non prende
 *
 * Non gli errori dentro i gestori d'evento, non quelli dentro `setTimeout`, non
 * le promesse rifiutate: quelli non passano dal disegno. Li raccolgono i due
 * ascoltatori globali in `main.tsx`, che scrivono nello stesso diario da questa
 * stessa parte. Le due strade sono complementari, non alternative.
 *
 * # Un recinto solo era troppo largo
 *
 * Il primo recinto stava attorno all'applicazione intera, e un pezzo che
 * cadeva — lo Studio che non si carica, lo spettro che non trova il WebGL, un
 * album con un dato che il disegno non si aspettava — si portava dietro la
 * navigazione, il lettore e la coda: al posto della finestra, una fascia rossa
 * e «Ricarica». Adesso ogni pagina, lo Studio, lo schermo intero e lo spettro
 * hanno il recinto loro (`dove`), e quel che cade cade da solo: al suo posto
 * la stessa fascia, con «Riprova» invece di «Ricarica», e il resto della
 * finestra che funziona. Quello attorno ad `App` resta, per quel che cade
 * fuori da tutti questi.
 */
import { Component, type ErrorInfo, type ReactNode } from "react";

import { ipc, testoErrore } from "./ipc";
import { t } from "./lingue";
import { Avviso } from "./parti/Avvisi";

interface Caduta {
  /** Il guasto, così com'è arrivato. `null` finché non ne arriva nessuno. */
  guasto: unknown;
  /** C'è stata una caduta. Un campo suo perché `null` è un guasto possibile. */
  caduto: boolean;
}

/**
 * Tiene in piedi la finestra quando quel che c'è dentro cade.
 *
 * Disegna con [`Avviso`] al livello `blocco` e con [`testoErrore`], che sanno
 * già trattare un `unknown` che non è un errore del nucleo — ed è il caso
 * normale qui: quel che arriva da React è quasi sempre un `TypeError` del
 * browser, non un codice del catalogo.
 */
export class RecintoErrori extends Component<
  {
    children: ReactNode;
    /**
     * Il nome del pezzo recintato, che finisce nel diario.
     *
     * Con questo il recinto è **locale**: la caduta prende il posto di quel
     * pezzo soltanto, e il tasto è «Riprova», che ridisegna il pezzo, e non
     * «Ricarica», che rifarebbe la finestra intera. Senza, è quello attorno
     * all'applicazione (`main.tsx`).
     */
    dove?: string | undefined;
    /**
     * Quando cambia, il recinto si rialza da solo.
     *
     * È la pagina: una pagina caduta non deve restare caduta quando se ne
     * apre un'altra. Una prop e non la `key` di React, che rimonterebbe tutto
     * il sotto a ogni cambio di pagina anche quando non è caduto niente.
     */
    chiave?: string | undefined;
    /**
     * Un'uscita accanto a «Riprova», per i pezzi che coprono il resto.
     *
     * Lo Studio e lo schermo intero stanno sopra la finestra: se cadono e
     * «Riprova» non basta — uno Studio il cui pacchetto non si è caricato
     * resta non caricato, perché `lazy` si ricorda il rifiuto — chi guarda
     * deve poter tornare alla finestra senza ricaricarla.
     */
    uscita?: { etichetta: string; fai: () => void } | undefined;
    /**
     * La fascia da sola, senza il riquadro `.vuoto` attorno.
     *
     * Per i pezzi che stanno in mezzo ad altro, come lo spettro dietro «In
     * riproduzione»: `.vuoto` è alto quanto il contenitore, e lì spingerebbe
     * fuori dallo schermo la copertina e i comandi che stanno ancora in piedi.
     */
    stretto?: boolean | undefined;
  },
  Caduta
> {
  override state: Caduta = { guasto: null, caduto: false };

  static getDerivedStateFromError(guasto: unknown): Caduta {
    return { guasto, caduto: true };
  }

  override componentDidUpdate(prima: Readonly<{ chiave?: string | undefined }>) {
    if (this.state.caduto && prima.chiave !== this.props.chiave) {
      this.setState({ guasto: null, caduto: false });
    }
  }

  override componentDidCatch(guasto: unknown, dove: ErrorInfo) {
    // Il componente in cui è successo e non lo stack: `componentStack` è
    // l'elenco dei componenti attraversati, cioè la cosa che serve davvero, e
    // la prima riga è quello che è caduto. Il nucleo taglia comunque a una riga
    // sola prima di scrivere — vedi `diario_annota` — quindi qui si manda già
    // la parte utile invece di far tagliare a caso.
    const dentro = (dove.componentStack ?? "").trim().split("\n")[0] ?? "";
    // Il pezzo nella parentesi del diario, quando il recinto è locale: «recinto
    // studio» dice dove guardare prima ancora di leggere il resto.
    const quale = this.props.dove === undefined ? "recinto" : `recinto ${this.props.dove}`;
    ipc.diarioAnnota(quale, `${testoErrore(guasto)} ${dentro}`.trim());
  }

  override render(): ReactNode {
    if (!this.state.caduto) return this.props.children;
    if (this.props.dove !== undefined) {
      const uscita = this.props.uscita;
      const fascia = (
        <Avviso
          livello="blocco"
          azione={
            <>
              <button
                type="button"
                className="bottone primario btn-accent"
                // Ridisegnare il pezzo e basta. Se il guasto sta nei suoi
                // dati ricadrà, e la fascia torna: il resto della finestra
                // intanto funziona, e cambiare pagina lo rialza da sé.
                onClick={() => this.setState({ guasto: null, caduto: false })}
              >
                {t("common.retry")}
              </button>
              {uscita !== undefined && (
                <button type="button" className="bottone" onClick={uscita.fai}>
                  {uscita.etichetta}
                </button>
              )}
            </>
          }
        >
          <strong>{t("crash.local")}</strong> {testoErrore(this.state.guasto)}
        </Avviso>
      );
      return this.props.stretto ? fascia : <div className="vuoto">{fascia}</div>;
    }
    return (
      <div className="vuoto">
        <Avviso
          livello="blocco"
          azione={
            <button
              type="button"
              className="bottone primario btn-accent"
              // Ricaricare la finestra e non azzerare lo stato del recinto: se
              // il guasto sta nello stato dell'applicazione — ed è il caso
              // normale, altrimenti non sarebbe caduta — ridisegnare gli stessi
              // dati rifà cadere tutto un istante dopo. Un tasto che non
              // funziona due volte su tre è peggio di nessun tasto.
              onClick={() => {
                window.location.reload();
              }}
            >
              {t("crash.reload")}
            </button>
          }
        >
          <strong>{t("crash.what")}</strong> {testoErrore(this.state.guasto)}
        </Avviso>
      </div>
    );
  }
}
