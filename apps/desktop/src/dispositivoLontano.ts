/**
 * Un dispositivo collegato, o una riga di un dispositivo, seguiti dal pannello:
 * gli hook che le testate e le righe della libreria usano per dire «risponde» o
 * «non risponde».
 *
 * Stavano nella pagina «Sfoglia» (`schermate/Dispositivo.tsx`), che con il
 * filtro per dispositivo non c'è più. Le regole restano quelle: chi legge si
 * ridisegna solo quando cambia **quel** dispositivo, non a ogni `lontano:stato`
 * (uno al minuto per dispositivo, più i cambi di via e di presenza: R13-11).
 */
import { useCallback, useMemo, useSyncExternalStore } from "react";

import { sceltaRicordata, ugualiInSuperficie } from "./archivio";
import { dataOraBreve } from "./formato";
import { dispositivoDellaRiga } from "./ipc/libreria";
import { t } from "./lingue";
import { archivioLontano, eTelefono } from "./lontano";
import { statoDellaRiga, type StatoDellaRiga } from "./origine";

/**
 * Di quale dispositivo è una riga della libreria e se risponde, o `null` per
 * una riga di qui.
 *
 * Una riga di qui non iscrive niente a niente: la chiave è `null`, la scelta
 * risponde `null` senza leggere il pannello, e una libreria senza dispositivi
 * non paga un confronto per riga.
 */
export function useStatoDellaRiga(percorso: string | null | undefined): StatoDellaRiga | null {
  const chiave = dispositivoDellaRiga(percorso);
  const scegli = useMemo(
    () =>
      sceltaRicordata(
        (stato: Parameters<typeof statoDellaRiga>[1]) =>
          chiave === null ? null : statoDellaRiga(percorso, stato),
        (a, b) => a === b || (a !== null && b !== null && ugualiInSuperficie(a, b)),
      ),
    // La scelta guarda solo la sorgente: si rifà quando cambia la riga.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [chiave, percorso],
  );
  const leggi = useCallback(() => scegli(archivioLontano.leggi()), [scegli]);
  return useSyncExternalStore(archivioLontano.iscrivi, leggi, leggi);
}

/** La riga sotto il nome: telefono o computer, se risponde, e se dal relay. */
export function sottotitoloDispositivo(
  dispositivo: {
    piattaforma: string;
    online: boolean;
    vistoMs: number | null;
    via: "diretto" | "relay" | null;
  } | null,
): string | undefined {
  if (dispositivo === null) return undefined;
  const tipo = t(eTelefono(dispositivo.piattaforma) ? "devices.kind.phone" : "devices.kind.computer");
  const stato = dispositivo.online
    ? t("devices.state.online")
    : dispositivo.vistoMs !== null
      ? t("devices.state.seen", { quando: dataOraBreve(dispositivo.vistoMs) })
      : t("devices.state.never");
  const via = dispositivo.online && dispositivo.via === "relay" ? ` · ${t("devices.via.relay")}` : "";
  return `${tipo} · ${stato}${via}`;
}

/**
 * Cosa dice di una riga il suo dispositivo, a chi non ne vede l'icona: da dove
 * viene, o che non risponde.
 */
export function etichettaDellaRiga(riga: StatoDellaRiga): string {
  if (riga.nome === "") return t("library.origin.gone");
  return riga.spenta
    ? t("library.origin.offline", { nome: riga.nome })
    : t("library.origin.from", { nome: riga.nome });
}
