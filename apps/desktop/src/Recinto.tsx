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
  { children: ReactNode },
  Caduta
> {
  override state: Caduta = { guasto: null, caduto: false };

  static getDerivedStateFromError(guasto: unknown): Caduta {
    return { guasto, caduto: true };
  }

  override componentDidCatch(guasto: unknown, dove: ErrorInfo) {
    // Il componente in cui è successo e non lo stack: `componentStack` è
    // l'elenco dei componenti attraversati, cioè la cosa che serve davvero, e
    // la prima riga è quello che è caduto. Il nucleo taglia comunque a una riga
    // sola prima di scrivere — vedi `diario_annota` — quindi qui si manda già
    // la parte utile invece di far tagliare a caso.
    const dentro = (dove.componentStack ?? "").trim().split("\n")[0] ?? "";
    ipc.diarioAnnota("recinto", `${testoErrore(guasto)} ${dentro}`.trim());
  }

  override render(): ReactNode {
    if (!this.state.caduto) return this.props.children;
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
