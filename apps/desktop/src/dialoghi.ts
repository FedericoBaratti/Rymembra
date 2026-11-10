/**
 * I selettori di file e cartelle, uguali sul computer e sul telefono.
 *
 * Sul computer sono quelli di `@tauri-apps/plugin-dialog`, e danno percorsi.
 * Su Android i selettori di sistema danno URI `content://`, che il nucleo non
 * sa leggere: lì si passa da `telefono.rs`, che traduce una cartella nel suo
 * percorso vero e **copia** un documento nella cache dell'app. Chi chiama
 * riceve un percorso in tutti e due i casi.
 *
 * Il salvataggio ha una forma sua, [`salvaCon`], perché su Android l'ordine è
 * rovesciato: prima il nucleo scrive il file in un posto suo, poi il ponte lo
 * copia dove chi guarda ha scelto. Con `save` del plugin il nucleo scriveva
 * direttamente nel percorso scelto.
 */
import { open as apriSulComputer, save as salvaSulComputer } from "@tauri-apps/plugin-dialog";

import { ipc } from "./ipc";
import { ANDROID } from "./piattaforma";

/** Un filtro per estensione, come lo vuole il plugin. */
export interface Filtro {
  name: string;
  extensions: string[];
}

/** Le opzioni di [`open`] che Rymembra usa. */
export interface OpzioniApri {
  directory?: boolean;
  multiple?: false;
  defaultPath?: string;
  filters?: Filtro[];
}

/**
 * Fa scegliere un file o una cartella. Il percorso, o `null` se chi guarda ha
 * annullato.
 *
 * Su Android i filtri non filtrano: il selettore di sistema filtra per tipo
 * MIME, e `.ryskin`, `.ryprofile` o `.m3u8` un tipo MIME non ce l'hanno — un
 * filtro li nasconderebbe tutti. Si mostrano tutti i documenti, e quello
 * sbagliato lo rifiuta il nucleo, con il suo messaggio, come sul computer.
 */
export async function open(opzioni: OpzioniApri = {}): Promise<string | null> {
  if (ANDROID) {
    return opzioni.directory ? ipc.telefonoScegliCartella() : ipc.telefonoApriDocumento([]);
  }
  const scelta = await apriSulComputer({ ...opzioni, multiple: false });
  return typeof scelta === "string" ? scelta : null;
}

/** Le opzioni di [`salvaCon`]. */
export interface OpzioniSalva {
  /** Il nome proposto, con l'estensione. */
  defaultPath: string;
  filters?: Filtro[];
  /** Il tipo MIME, per il selettore di Android. Di serie, un file qualunque. */
  tipo?: string;
}

/**
 * Fa scegliere dove salvare, e fa scrivere il file a `scrivi`.
 *
 * Restituisce quel che `scrivi` restituisce, o `null` se chi guarda ha
 * annullato. `dove` è il nome da mostrare a chi ha salvato: il percorso sul
 * computer, solo il nome sul telefono — un URI non dice niente a nessuno.
 */
export async function salvaCon<T>(
  opzioni: OpzioniSalva,
  scrivi: (percorso: string) => Promise<T>,
): Promise<{ esito: T; dove: string } | null> {
  if (ANDROID) {
    const nome = opzioni.defaultPath;
    const provvisorio = await ipc.telefonoFileTemporaneo(nome);
    const esito = await scrivi(provvisorio);
    const salvato = await ipc.telefonoSalvaDocumento(
      nome,
      opzioni.tipo ?? "application/octet-stream",
      provvisorio,
    );
    return salvato ? { esito, dove: nome } : null;
  }
  const scelta = await salvaSulComputer({
    defaultPath: opzioni.defaultPath,
    ...(opzioni.filters ? { filters: opzioni.filters } : {}),
  });
  if (typeof scelta !== "string") return null;
  return { esito: await scrivi(scelta), dove: scelta };
}
