/**
 * Lo sfondo di una superficie, a livelli.
 *
 * # Perché è la parte che mancava
 *
 * `parts.<nome>.background` è un array di effetti, e finora lo Studio lo leggeva
 * soltanto per sommarne il costo: per aggiungere una griglia di punti a una
 * scheda bisognava scendere nella vista Documento e scrivere l'oggetto a mano.
 * Il che vuol dire che la vista a controlli non era una seconda tastiera sullo
 * stesso documento — era una tastiera con meno tasti.
 *
 * # Il costo accanto al nome, sempre
 *
 * Ogni livello porta il suo peso, e l'elenco da cui si sceglie ce l'ha già
 * accanto a ciascuna voce: si vede quanto costa **prima** di spenderlo. È la
 * terza regola dello Studio, e senza questo elenco non aveva un posto dove
 * succedere.
 *
 * # Perché si riscrive l'array intero
 *
 * `patch.ts` non indicizza gli array, e non serve che lo faccia: aggiungere,
 * togliere e riordinare producono tutti e tre un array nuovo, e scriverlo in un
 * colpo solo è anche l'unico modo di non lasciare il documento in uno stato
 * intermedio a metà di un riordino.
 */
import { useState } from "react";

import type { EffettoRegistro, TokenRegistro } from "../ipc";
import { Icona } from "../parti/Icone";
import { coloreCosto, costoDi, type Livello, nomeEffetto, ritrattoEffetto } from "./valori";

export function Livelli({
  livelli,
  effetti,
  tokens,
  tavolozza,
  budget,
  onCambia,
}: {
  livelli: readonly Livello[];
  effetti: readonly EffettoRegistro[];
  tokens: readonly TokenRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  budget: number;
  /** L'array nuovo. Vuoto vuol dire «togli la dichiarazione». */
  onCambia: (livelli: Livello[]) => void;
}) {
  const [apertoElenco, setApertoElenco] = useState(false);
  const [preso, setPreso] = useState<number | null>(null);

  /** Solo gli effetti che si possono impilare come sfondo. */
  const impilabili = effetti.filter((e) => e.target === "background");

  const aggiungi = (effetto: EffettoRegistro) => {
    try {
      onCambia([...livelli, JSON.parse(effetto.esempio) as Livello]);
    } catch {
      // L'esemplare viene dal nucleo e non da qui: se non è JSON, il posto dove
      // aggiustarlo è `esempio()` in `studio.rs`, non un ripiego inventato ora.
    }
    setApertoElenco(false);
  };

  const togli = (indice: number) =>
    onCambia(livelli.filter((_, i) => i !== indice));

  /** Sposta un livello, e restituisce l'array nuovo. */
  const sposta = (da: number, a: number) => {
    if (da === a) return;
    const nuovi = [...livelli];
    const [tolto] = nuovi.splice(da, 1);
    if (tolto === undefined) return;
    nuovi.splice(a, 0, tolto);
    onCambia(nuovi);
  };

  return (
    <div className="livelli">
      <div className="testa-sezione">
        <span className="titolino">Sfondo · livelli</span>
        <span className="da-dove">dal più basso</span>
      </div>

      <div className="pila">
        {livelli.map((livello, indice) => {
          const nome = nomeEffetto(livello) ?? "?";
          const costo = costoDi(livello, effetti);
          return (
            <div
              // L'indice è la chiave perché è l'identità: due `solid` nella
              // stessa pila sono due livelli diversi e nient'altro li distingue.
              key={indice}
              className="livello"
              draggable
              data-preso={preso === indice || undefined}
              onDragStart={(e) => {
                setPreso(indice);
                e.dataTransfer.effectAllowed = "move";
              }}
              onDragOver={(e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (preso !== null) sposta(preso, indice);
                setPreso(null);
              }}
              onDragEnd={() => setPreso(null)}
            >
              <span className="maniglia" aria-hidden="true">
                <Icona nome="i-grip" dim={12} />
              </span>
              <span
                className="ritratto"
                style={{ background: ritrattoEffetto(livello, tokens, tavolozza) }}
                aria-hidden="true"
              />
              <code className="quale">{nome}</code>
              <span className="peso" style={{ color: coloreCosto(costo, budget) }}>
                {costo}
              </span>
              <button
                type="button"
                className="via icon-btn"
                aria-label={`Togli il livello ${nome}`}
                onClick={() => togli(indice)}
              >
                <Icona nome="i-x" dim={12} />
              </button>
            </div>
          );
        })}

        <button
          type="button"
          className="aggiungi-livello"
          aria-expanded={apertoElenco}
          onClick={() => setApertoElenco((prima) => !prima)}
        >
          <Icona nome={apertoElenco ? "i-chev-u" : "i-plus"} dim={13} />
          <span>Aggiungi livello</span>
          <span className="quanti">{impilabili.length} effetti</span>
        </button>

        {apertoElenco && (
          <div className="elenco-effetti" role="menu">
            {impilabili.map((effetto) => (
              <button
                key={effetto.name}
                type="button"
                role="menuitem"
                className="un-effetto"
                onClick={() => aggiungi(effetto)}
              >
                <code>{effetto.name}</code>
                {/* Il peso **prima** di spenderlo: è tutto il punto della terza
                    regola, e un elenco senza questo numero costringerebbe a
                    scoprirlo aggiungendo. */}
                <span className="peso" style={{ color: coloreCosto(effetto.cost, budget) }}>
                  {effetto.cost}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
