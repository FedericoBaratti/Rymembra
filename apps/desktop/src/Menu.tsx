/**
 * Il menù che si apre col tasto destro.
 *
 * Esiste perché «riproduci dopo» e «accoda» non hanno un posto sulla riga:
 * l'elenco ha già sei colonne, e due tasti in più per ogni brano sarebbero due
 * tasti da guardare mille volte per usarli una. Da qui passerà anche «aggiungi
 * a playlist», che è la stessa forma di comando.
 */
import { useEffect, useRef } from "react";

/** Una voce del menù. */
export interface Voce {
  etichetta: string;
  azione: () => void;
}

/** Dove si è aperto e su cosa. */
export interface Apertura {
  x: number;
  y: number;
  voci: Voce[];
}

export function Menu({
  apertura,
  onChiudi,
}: {
  apertura: Apertura;
  onChiudi: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const suTasto = (e: KeyboardEvent) => {
      if (e.key === "Escape") onChiudi();
    };
    window.addEventListener("keydown", suTasto);
    return () => window.removeEventListener("keydown", suTasto);
  }, [onChiudi]);

  // Il fuoco entra nel menù appena si apre: chi naviga da tastiera deve poterlo
  // percorrere senza prima ritrovare con Tab il punto in cui si è aperto.
  useEffect(() => {
    menu.current?.querySelector("button")?.focus();
  }, []);

  return (
    <>
      {/* Il velo prende il clic di chiusura, compreso il destro: senza, un
          secondo tasto destro altrove aprirebbe un menù col primo ancora su. */}
      <div
        className="velo"
        onClick={onChiudi}
        onContextMenu={(e) => {
          e.preventDefault();
          onChiudi();
        }}
      />
      <div
        ref={menu}
        className="menu menu-pop"
        role="menu"
        /* Il menù si ancora in alto a sinistra del puntatore e si ribalta da
           solo con `translate` quando sborderebbe: calcolarlo in JavaScript
           vorrebbe dire misurarlo dopo averlo disegnato, cioè un fotogramma nel
           posto sbagliato. */
        style={{
          left: `min(${apertura.x}px, 100vw - 220px)`,
          top: `min(${apertura.y}px, 100vh - ${apertura.voci.length * 34 + 16}px)`,
        }}
      >
        {apertura.voci.map((voce) => (
          <button
            key={voce.etichetta}
            type="button"
            role="menuitem"
            onClick={() => {
              voce.azione();
              onChiudi();
            }}
          >
            {voce.etichetta}
          </button>
        ))}
      </div>
    </>
  );
}
