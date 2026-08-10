/**
 * La finestrella che battezza un tema nuovo.
 *
 * # Perché quattro domande e non una
 *
 * Perché `meta` ha quattro campi obbligatori — `name`, `author`, `version` — e
 * uno facoltativo che la scheda della skin mostra sotto il nome. Chiedere solo
 * il nome vorrebbe dire scrivere «Aether» come autore di un tema che l'utente ha
 * fatto da sé, e lasciare a lui il compito di scoprire che si corregge nella
 * scheda «Documento» dello Studio. La versione è l'unica che non si chiede: un
 * tema nuovo è `1.0.0` e non c'è una seconda risposta possibile.
 *
 * # Perché la base si sceglie qui
 *
 * Perché è la domanda che non si può più fare dopo. Nome, autore e descrizione
 * si riscrivono in qualunque momento; da cosa si è partiti no — a Studio aperto
 * la scelta è già stata fatta, e cambiarla vuol dire ricominciare.
 *
 * L'identificatore invece **non** si chiede: si deriva dal nome e si mostra
 * mentre lo si scrive. È un nome di file e un selettore CSS, non una cosa da
 * comporre a mano, e mostrarlo senza chiederlo è quel che serve a non
 * sorprendere chi poi lo ritrova nella cartella delle skin.
 */
import { useEffect, useRef, useState } from "react";

import type { VoceSkin } from "./ipc";
import { idDa, type DatiTema } from "./studio/nuovo";

/** Le tre fasce di una skin, col ripiego di `Impostazioni`. */
function bande(skin: VoceSkin): readonly string[] {
  return skin.anteprima.length === 3
    ? skin.anteprima
    : ["var(--color-surface-0)", "var(--color-surface-2)", "var(--accent)"];
}

export function NuovoTema({
  skin,
  onCrea,
  onChiudi,
}: {
  /** Le skin fra cui scegliere la base, e gli id già presi. */
  skin: VoceSkin[];
  onCrea: (dati: DatiTema) => void;
  onChiudi: () => void;
}) {
  const [nome, setNome] = useState("");
  const [autore, setAutore] = useState("");
  const [descrizione, setDescrizione] = useState("");
  const [base, setBase] = useState(
    () => skin.find((s) => s.attiva)?.id ?? skin[0]?.id ?? "plain",
  );
  const campo = useRef<HTMLInputElement>(null);

  useEffect(() => {
    campo.current?.focus();
  }, []);

  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") onChiudi();
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  /** Le frecce girano dentro il gruppo, come nel segmentato. */
  const daTastiera = (e: React.KeyboardEvent, indice: number) => {
    // È una griglia che va a capo, quindi non c'è un «sopra» che si possa dire
    // a colpo sicuro: le quattro frecce scorrono l'elenco, avanti e indietro.
    const passo =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (passo === 0) return;
    e.preventDefault();
    // Il modulo con l'addizione: in JavaScript `-1 % 3` fa `-1`, non `2`.
    const prossima = skin[(indice + passo + skin.length) % skin.length];
    if (prossima) setBase(prossima.id);
  };

  const pulito = nome.trim();
  const vuoto = pulito.length === 0;
  // Derivato a ogni carattere: è il campo che l'utente non compila e che deve
  // comunque poter prevedere.
  const id = vuoto ? "" : idDa(pulito, skin.map((s) => s.id));

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <form
        className="finestrella nuovo-tema"
        aria-label="Crea un tema"
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          if (vuoto) return;
          onCrea({
            id,
            nome: pulito,
            autore: autore.trim().length > 0 ? autore.trim() : "Io",
            descrizione: descrizione.trim(),
            base,
          });
        }}
      >
        <h2>Crea un tema</h2>

        <div className="campo-con-nota">
          <label>
            Nome
            <input
              ref={campo}
              className="campo"
              value={nome}
              spellCheck={false}
              placeholder="Notturno"
              onChange={(e) => setNome(e.target.value)}
            />
          </label>
          {/* Fuori dalla `label` di proposito: dentro finirebbe nel nome
              accessibile del campo, e chi non vede il modulo si sentirebbe
              leggere «Nome l'identificatore viene dal nome». */}
          <p className="nota-id">
            {vuoto ? "l'identificatore viene dal nome" : `id: ${id}`}
          </p>
        </div>

        <label>
          Autore
          <input
            className="campo"
            value={autore}
            spellCheck={false}
            placeholder="Io"
            onChange={(e) => setAutore(e.target.value)}
          />
        </label>

        <label>
          Descrizione <span className="facoltativo">facoltativa</span>
          <input
            className="campo"
            value={descrizione}
            placeholder="A cosa somiglia"
            onChange={(e) => setDescrizione(e.target.value)}
          />
        </label>

        <div className="scelta-base">
          <span className="etichetta-gruppo" id="da-quale-skin">
            Parti da
          </span>
          {/* `radiogroup` e non un elenco di bottoni, per la stessa ragione del
              segmentato: sono opzioni che si escludono, e il tabulatore deve
              attraversare il gruppo in una fermata sola invece che in una per
              skin installata. */}
          <div className="basi-tema" role="radiogroup" aria-labelledby="da-quale-skin">
            {skin.map((s, indice) => {
              const scelta = s.id === base;
              return (
                <button
                  key={s.id}
                  type="button"
                  className="base"
                  role="radio"
                  aria-checked={scelta}
                  data-scelta={scelta || undefined}
                  tabIndex={scelta ? 0 : -1}
                  onClick={() => setBase(s.id)}
                  onKeyDown={(e) => daTastiera(e, indice)}
                >
                  <span className="bande" aria-hidden="true">
                    {bande(s).map((colore, i) => (
                      <span key={i} style={{ background: colore }} />
                    ))}
                  </span>
                  <span className="nome-skin">{s.nome}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            Annulla
          </button>
          <button type="submit" className="bottone primario" disabled={vuoto}>
            Crea e apri lo Studio
          </button>
        </div>
      </form>
    </div>
  );
}
