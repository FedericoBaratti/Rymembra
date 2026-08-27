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

  // Il fuoco nel campo, col testo già selezionato: rinominare vuol dire quasi
  // sempre sostituire, non aggiungere in coda.
  useEffect(() => {
    campo.current?.select();
  }, []);

  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") onChiudi();
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  const vuoto = testo.trim().length === 0;

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <form
        className="finestrella stretta glass-modal"
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
