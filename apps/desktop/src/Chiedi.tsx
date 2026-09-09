/**
 * Una domanda con una risposta sola.
 *
 * Esiste perché `window.prompt` non c'è: in un webview Tauri è disabilitata, e
 * anche dove funziona è una finestra del sistema operativo che non conosce i
 * token della skin. Serve a creare e a rinominare una playlist, che sono la
 * stessa domanda fatta due volte.
 */
import { useEffect, useRef, useState } from "react";
import { t } from "./lingue";

export function Chiedi({
  titolo,
  etichetta,
  iniziale = "",
  conferma,
  onRispondi,
  onChiudi,
}: {
  titolo: string;
  etichetta: string;
  iniziale?: string;
  conferma: string;
  onRispondi: (risposta: string) => void;
  onChiudi: () => void;
}) {
  const [testo, setTesto] = useState(iniziale);
  const campo = useRef<HTMLInputElement>(null);
  const finestrella = useRef<HTMLFormElement>(null);
  const chiAveva = useRef<HTMLElement | null>(null);

  /**
   * Il fuoco, all'apertura e alla chiusura.
   *
   * All'apertura va nel campo col testo già selezionato: rinominare vuol dire
   * quasi sempre sostituire, non aggiungere in coda. Alla chiusura si prova a
   * rimetterlo dov'era, e il ripristino vale quando chi ha aperto è un elemento
   * focalizzabile ancora montato: un tasto di `parti/Navigazione.tsx`, un
   * comando della barra. Quando la domanda arriva da una voce di `Menu` non lo
   * è: quel bottone è già staccato dal DOM nel momento in cui questo componente
   * si monta, e quel che si legge è il `<body>`. La guardia rende esplicito il
   * no-op invece di fingere un ritorno — il fuoco resta dove sarebbe caduto
   * comunque, cioè sul `<body>`, come prima.
   *
   * Un «ancoraggio» esplicito passato da chi apre coprirebbe anche quel caso,
   * ma cambierebbe le prop di `Chiedi` e tutti i suoi chiamanti: fuori dal
   * perimetro di un raffinamento a comportamento invariato. È il candidato
   * successivo.
   *
   * Chi aveva il fuoco va letto **prima** di prenderglielo.
   */
  useEffect(() => {
    chiAveva.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    campo.current?.select();
    return () => {
      const chi = chiAveva.current;
      if (chi && chi !== document.body && chi.isConnected) chi.focus();
    };
  }, []);

  /**
   * Escape chiude, e il Tab resta dentro.
   *
   * Un dialogo senza trappola lascia il Tab uscire dietro al velo: il fuoco
   * finisce sui comandi della pagina coperta, che si vedono ma non rispondono
   * al clic, e da lì nessun tasto riporta indietro. Qui i fuochi sono tre — il
   * campo e i due tasti — e il ciclo li richiude ad anello. Il tasto di
   * conferma esce dal giro finché è disabilitato, che è già quel che fa il
   * browser da sé: `:not(:disabled)` dice al selettore la stessa cosa.
   */
  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onChiudi();
        return;
      }
      if (e.key !== "Tab") return;
      const dentro = finestrella.current;
      if (!dentro) return;
      const fuochi = Array.from(
        dentro.querySelectorAll<HTMLElement>("input, button:not(:disabled)"),
      );
      const primo = fuochi[0];
      const ultimo = fuochi[fuochi.length - 1];
      if (!primo || !ultimo) return;
      const dove = document.activeElement;
      const capo = e.shiftKey ? primo : ultimo;
      if (!dentro.contains(dove) || dove === capo) {
        e.preventDefault();
        (e.shiftKey ? ultimo : primo).focus();
      }
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  const vuoto = testo.trim().length === 0;

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <form
        ref={finestrella}
        className="finestrella stretta glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={titolo}
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          if (vuoto) return;
          onRispondi(testo.trim());
        }}
      >
        <h2>{titolo}</h2>
        <label>
          {etichetta}
          <input
            ref={campo}
            className="campo field-input"
            value={testo}
            spellCheck={false}
            onChange={(e) => setTesto(e.target.value)}
          />
        </label>
        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="bottone primario" disabled={vuoto}>
            {conferma}
          </button>
        </div>
      </form>
    </div>
  );
}
