/**
 * Il filtro per dispositivo: «Tutti · Questo dispositivo · <nome>».
 *
 * È quel che prima era la pagina «Sfoglia»: con i dispositivi collegati la
 * libreria è una sola, e il filtro dice da dove si guarda (`origine.ts`). Sta
 * sopra gli elenchi che ne dipendono — Album, Artisti, Brani, Preferiti e la
 * ricerca — e non c'è finché non c'è un dispositivo collegato.
 *
 * Sul computer sono un gruppo di scelte (`Segmentato`, un `radiogroup`: le
 * opzioni si escludono, e le frecce girano dentro il gruppo); sul telefono una
 * fila di pillole che scorre di lato, come quella della Libreria
 * (`PilloleLibreria`), con `aria-pressed` perché ognuna è un pulsante.
 *
 * Legge il pannello da sé e non da una prop: la barra della testata è un
 * disegno che non deve rifarsi a ogni `lontano:stato`.
 */
import { useLayoutEffect, useRef } from "react";

import { useArchivio } from "../archivio";
import type { OrigineLibreria } from "../ipc/libreria";
import { t } from "../lingue";
import { archivioLontano } from "../lontano";
import { chiaveDiOrigine, pilloleOrigine, type PillolaOrigine } from "../origine";
import { MOBILE } from "../piattaforma";
import { Segmentato } from "./Segmentato";

function etichettaDi(pillola: PillolaOrigine): string {
  if (pillola.dispositivo !== null) return pillola.dispositivo.nome;
  return pillola.origine.tipo === "locale" ? t("library.origin.local") : t("library.origin.all");
}

/** Le pillole come vengono dal pannello: uguali se dicono le stesse cose. */
function ugualiPillole(a: readonly PillolaOrigine[], b: readonly PillolaOrigine[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (p, i) =>
        chiaveDiOrigine(p.origine) === chiaveDiOrigine(b[i]?.origine ?? p.origine) &&
        p.dispositivo?.nome === b[i]?.dispositivo?.nome,
    )
  );
}

export function PilloleOrigine({
  origine,
  onOrigine,
}: {
  origine: OrigineLibreria;
  onOrigine: (origine: OrigineLibreria) => void;
}) {
  const pillole = useArchivio(archivioLontano, pilloleOrigine, ugualiPillole);
  const fila = useRef<HTMLDivElement>(null);
  const scelta = chiaveDiOrigine(origine);
  useLayoutEffect(() => {
    // La pillola accesa si porta in vista da sé, come nella Libreria: con la
    // fila scorsa oltre l'ultimo dispositivo, tornare a «Tutti» la lascerebbe
    // fuori dallo schermo.
    const accesa = fila.current?.querySelector<HTMLElement>("[aria-pressed='true']");
    accesa?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [scelta]);
  if (pillole.length === 0) return null;

  if (!MOBILE) {
    return (
      <Segmentato<string>
        etichetta={t("library.origin.aria")}
        scelta={scelta}
        voci={pillole.map((p) => ({ chiave: chiaveDiOrigine(p.origine), etichetta: etichettaDi(p) }))}
        onScegli={(chiave) => {
          const scelta = pillole.find((p) => chiaveDiOrigine(p.origine) === chiave);
          if (scelta !== undefined) onOrigine(scelta.origine);
        }}
      />
    );
  }
  return (
    <div ref={fila} className="pillole-libreria" role="group" aria-label={t("library.origin.aria")}>
      {pillole.map((p) => {
        const chiave = chiaveDiOrigine(p.origine);
        return (
          <button
            key={chiave}
            type="button"
            className="pillola-libreria"
            aria-pressed={chiave === scelta}
            onClick={() => onOrigine(p.origine)}
          >
            {etichettaDi(p)}
          </button>
        );
      })}
    </div>
  );
}
