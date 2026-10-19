/**
 * L'ospite delle finestrelle: un posto solo per quel che fanno tutte.
 *
 * # Perché adesso un componente
 *
 * `finestrella.ts` spiega perché per due anni le finestrelle hanno chiamato un
 * hook invece di stare dentro un componente: il loro markup non si somigliava,
 * e quel che avevano in comune era il fuoco. Nella 3.0 quel che hanno in comune
 * è cresciuto, e le copie hanno cominciato a divergere nel modo in cui le
 * copie divergono: due finestrelle d'importazione erano rimaste **senza**
 * trappola del fuoco, ogni velo chiudeva anche alla fine di una selezione
 * trascinata fuori, e la pagina dietro restava raggiungibile da un lettore di
 * schermo in tutte e dieci. Qui quelle cose stanno una volta:
 *
 * - `role="dialog"` (o `alertdialog`, per le domande che precedono un gesto
 *   distruttivo), `aria-modal`, e il nome dal titolo con `aria-labelledby`;
 * - il fuoco che entra, gira col Tab senza uscire e torna a chi l'aveva, ed
 *   Escape che chiude — tutto da `useFinestrella`;
 * - il resto della finestra `inert` finché la finestrella è aperta
 *   (`useVeloInerte`);
 * - il velo che chiude solo un clic nato **e** finito su di lui (`nuovoVelo`
 *   in `modale.ts`, che dice perché).
 *
 * # Quel che resta a chi la usa
 *
 * Le classi, il contenuto, i tasti. Le classi arrivano scritte per intero da
 * chi chiama — `classe="finestrella stretta glass-modal"` — e non composte qui:
 * sono il contratto con le skin (`glass-modal` è una parte del registro di
 * `rymembra-skin`), e `strumenti/classi.js` le legge dove sono scritte. Il velo
 * invece è sempre lo stesso, e porta le sue qui.
 *
 * Il titolo diventa l'`<h2>` in cima, perché è lui a dare il nome al dialogo:
 * scriverlo due volte — nell'`aria-label` e nel titolo — era il modo in cui il
 * nome e il titolo si separavano. Le finestrelle a più passi, il cui titolo
 * dice il passo («Cosa verrebbe importato») e non la finestrella, passano un
 * `etichetta` stabile, che allora vince.
 */
import {
  useId,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";

import { useFinestrella, useVeloInerte } from "./finestrella";
import { nuovoVelo } from "./modale";
import { Presenza } from "./presenza";

/** Escape che non chiude: la finestrella ha detto di no, per adesso. */
const restaAperta = () => undefined;

export function Dialogo({
  titolo,
  etichetta,
  descrizione,
  ruolo = "dialog",
  classe,
  onChiudi,
  chiudibile = true,
  veloChiude,
  occupato,
  onInvia,
  children,
}: {
  /** Il titolo: l'`<h2>` in cima, e il nome del dialogo. */
  titolo: ReactNode;
  /**
   * Un nome stabile, quando il titolo dice il passo e non la finestrella.
   *
   * Con questo il nome viene da `aria-label` invece che dal titolo.
   */
  etichetta?: string | undefined;
  /**
   * La frase che dice cosa si sta decidendo: un `<p>` sotto il titolo, legato
   * con `aria-describedby`. È quel che un lettore di schermo legge subito dopo
   * il nome di un `alertdialog`, quindi è la domanda e non il contorno.
   */
  descrizione?: ReactNode;
  /** `alertdialog` per le domande che precedono un gesto che toglie qualcosa. */
  ruolo?: "dialog" | "alertdialog" | undefined;
  /** Le classi della finestrella, per intero: sono il contratto con le skin. */
  classe: string;
  /** Quel che fanno Escape e il velo. */
  onChiudi: () => void;
  /**
   * Si può chiudere adesso?
   *
   * Spento, Escape non fa niente e il velo nemmeno: è il caso di una
   * finestrella che sta scrivendo e che, chiusa a metà, lascerebbe un lavoro
   * senza nessuno che ne racconti l'esito.
   */
  chiudibile?: boolean | undefined;
  /**
   * Il velo chiude? Di serie quel che dice `chiudibile`.
   *
   * Separato perché due finestrelle d'importazione lasciano funzionare Escape
   * mentre leggono — è un gesto deliberato — e spengono il velo, che un clic
   * distratto accanto alla finestrella prenderebbe per un «chiudi».
   */
  veloChiude?: boolean | undefined;
  /** `aria-busy`: sta lavorando, e quel che si vede sta per cambiare. */
  occupato?: boolean | undefined;
  /** Se c'è, la finestrella è un `<form>`, e Invio la manda qui. */
  onInvia?: (() => void) | undefined;
  children: ReactNode;
}) {
  const idTitolo = useId();
  const idDescrizione = useId();
  // L'hook prima di `useVeloInerte`, e non per gusto: legge chi aveva il fuoco
  // in un effetto di disposizione, e deve farlo prima che la pagina dietro
  // diventi inerte. Vedi `finestrella.ts`.
  const radice = useFinestrella<HTMLElement>(chiudibile ? onChiudi : restaAperta);
  const velo = useRef<HTMLDivElement>(null);
  useVeloInerte(velo);
  const [gesto] = useState(nuovoVelo);
  const chiudeIlVelo = veloChiude ?? chiudibile;

  const contenuto = (
    <>
      <h2 id={idTitolo}>{titolo}</h2>
      {descrizione !== undefined && <p id={idDescrizione}>{descrizione}</p>}
      {children}
    </>
  );
  const nome =
    etichetta === undefined
      ? { "aria-labelledby": idTitolo }
      : { "aria-label": etichetta };
  const descritta =
    descrizione !== undefined ? { "aria-describedby": idDescrizione } : {};

  return (
    <Presenza parte="modal-veil">
      <div
        ref={velo}
        className="velo scuro modal-veil"
        onPointerDown={(e) => gesto.premuto(e.target, e.currentTarget)}
        onClick={(e) => {
          if (gesto.clic(e.target, e.currentTarget) && chiudeIlVelo) onChiudi();
        }}
      >
        <Presenza parte="glass-modal">
          {onInvia === undefined ? (
            <div
              ref={radice as RefObject<HTMLDivElement | null>}
              className={classe}
              role={ruolo}
              aria-modal="true"
              aria-busy={occupato}
              {...nome}
              {...descritta}
              // Il clic resta qui, come prima del velo nuovo: chi sta sopra —
              // la pagina di Cartelle, per la sua conferma — non deve vedere i
              // clic sui tasti della finestrella.
              onClick={(e) => e.stopPropagation()}
            >
              {contenuto}
            </div>
          ) : (
            <form
              ref={radice as RefObject<HTMLFormElement | null>}
              className={classe}
              role={ruolo}
              aria-modal="true"
              aria-busy={occupato}
              {...nome}
              {...descritta}
              onClick={(e) => e.stopPropagation()}
              onSubmit={(e) => {
                e.preventDefault();
                onInvia();
              }}
            >
              {contenuto}
            </form>
          )}
        </Presenza>
      </div>
    </Presenza>
  );
}
