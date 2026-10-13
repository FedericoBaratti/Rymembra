/**
 * «Vai»: da un percorso del documento alla cosa, nello Studio.
 *
 * La usano i Problemi, gli elenchi «usato da» e la chat: dato un percorso
 * puntato, sceglie quel che nomina, con lo stato e il tema giusti, e porta la
 * tela dove la parte si vede. Una sola strada, così «vai» fa la stessa cosa
 * da qualunque punto lo si prema.
 */
import { useCallback } from "react";

import { useAzioniStudio } from "./contesto";
import { destinazioneDi } from "./sentieri";

export function useVai(): (percorso: string) => boolean {
  const { manda, registro } = useAzioniStudio();
  return useCallback(
    (percorso: string) => {
      const d = destinazioneDi(
        percorso,
        registro.tokens.map((t) => t.id),
      );
      if (d === null) return false;
      manda({ tipo: "scegli", selezione: d.selezione });
      const temiContano = ["parte", "token", "colore", "motivo"].includes(d.selezione.tipo);
      if (d.tema !== undefined || temiContano) {
        manda({ tipo: "imposta", campi: { tema: d.tema ?? "base" } });
      }
      if (d.stato !== undefined) manda({ tipo: "stato", stato: d.stato });
      if (d.ispettore !== undefined) manda({ tipo: "imposta", campi: { ispettore: d.ispettore } });
      if (d.selezione.tipo === "parte") manda({ tipo: "mostra", parte: d.selezione.nome });
      if (d.selezione.tipo === "animazione") manda({ tipo: "imposta", campi: { dock: "timeline" } });
      return true;
    },
    [manda, registro],
  );
}
