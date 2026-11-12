/**
 * Il permesso di leggere la musica, sul telefono.
 *
 * Su Android un'app non vede i file della memoria condivisa finché chi la usa
 * non lo concede, e il primo avvio cercherebbe la musica in cartelle che
 * risultano vuote — «Nessuna musica trovata», con la musica lì. Per questo il
 * primo avvio, sul telefono, chiede prima l'accesso e poi cerca.
 *
 * I livelli sono due, perché Android ne ha due: i permessi «media» (un dialogo,
 * bastano ai brani e alle copertine) e «Accesso a tutti i file» (una pagina
 * delle impostazioni, serve ai `.lrc`, ai `.cue` e alle eliminazioni).
 * Si chiedono tutti e due con un tasto solo; chi concede soltanto il primo può
 * continuare lo stesso, sapendo che cosa resta fuori.
 *
 * Non solo al primo avvio: «Lo faccio dopo» resta, e chi lo sceglie — o dice di
 * no al dialogo di Android — lo ritrova negli stati vuoti della libreria
 * ([notaDelPermesso]) e in Impostazioni › Cartelle e scansione
 * ([PermessoMusica]). La regola di che cosa chiedere dove sta in
 * `permessoMusica.ts`.
 *
 * Fuori da Android l'accesso è sempre «pieno» e qui non si disegna niente.
 */
import { useCallback, useEffect, useState } from "react";

import { ipc, type PermessiTelefono } from "../ipc";
import { t } from "../lingue";
import { ANDROID } from "../piattaforma";
import { cosaChiedere, type StatoAccesso } from "./permessoMusica";

export type { StatoAccesso };

/** Lo stato dell'accesso, e i due gesti che lo cambiano. */
export interface Accesso {
  /** Si può cercare la musica. */
  pronto: boolean;
  stato: StatoAccesso;
  /** Un dialogo o la pagina delle impostazioni sono aperti. */
  inCorso: boolean;
  /** Chiede i permessi del dialogo e poi «Accesso a tutti i file». */
  consenti: () => void;
  /** Va avanti con i soli permessi media. */
  soloBrani: () => void;
}

function statoDi(p: PermessiTelefono): StatoAccesso {
  if (p.tuttiIFile) return "pieno";
  return p.audio ? "solo-media" : "manca";
}

export function useAccessoTelefono(): Accesso {
  const [stato, setStato] = useState<StatoAccesso>(ANDROID ? "verifica" : "pieno");
  const [soloMedia, setSoloMedia] = useState(false);
  const [inCorso, setInCorso] = useState(false);

  const rileggi = useCallback(
    () => ipc.telefonoPermessi().then((p) => setStato(statoDi(p))),
    [],
  );

  useEffect(() => {
    if (!ANDROID) return;
    // Un ponte che non risponde non deve fermare il primo avvio: si va
    // avanti come se l'accesso ci fosse, e la cartella vuota lo dirà.
    void rileggi().catch(() => setStato("pieno"));
    // «Accesso a tutti i file» si concede in una pagina delle impostazioni, e
    // da lì si torna senza un risultato da aspettare: lo si rilegge quando
    // l'app torna davanti.
    const torna = () => {
      if (document.visibilityState === "visible") void rileggi().catch(() => {});
    };
    document.addEventListener("visibilitychange", torna);
    window.addEventListener("focus", torna);
    return () => {
      document.removeEventListener("visibilitychange", torna);
      window.removeEventListener("focus", torna);
    };
  }, [rileggi]);

  const consenti = useCallback(() => {
    setInCorso(true);
    void (async () => {
      const dopo = await ipc.telefonoChiediPermessi();
      setStato(statoDi(dopo));
      if (!dopo.tuttiIFile) await ipc.telefonoChiediTuttiIFile();
    })()
      .catch(() => {})
      .finally(() => setInCorso(false));
  }, []);

  return {
    pronto: stato === "pieno" || (stato === "solo-media" && soloMedia),
    stato,
    inCorso,
    consenti,
    soloBrani: () => setSoloMedia(true),
  };
}

/**
 * La riga in più di uno stato vuoto della libreria, quando a svuotarla è il
 * permesso che manca: lo dice, e lo chiede. `undefined` negli altri casi, così
 * lo stato vuoto non disegna una riga vuota.
 *
 * Prende l'accesso da chi la usa, e non lo legge da sé: lo stato vuoto si
 * disegna dentro `App`, che di accessi ne tiene uno solo per tutte le pagine.
 */
export function notaDelPermesso(accesso: Accesso): React.ReactNode | undefined {
  if (cosaChiedere(accesso.stato, "vuoto") === "niente") return undefined;
  return (
    <>
      {t("telefono.accesso.manca")}{" "}
      <button
        type="button"
        className="bottone minuto"
        onClick={accesso.consenti}
        disabled={accesso.inCorso}
      >
        {t("telefono.accesso.consenti")}
      </button>
    </>
  );
}

/**
 * Il permesso nella scheda delle cartelle sorvegliate: che cosa manca, e il
 * tasto che lo chiede. Anche a chi ha i soli brani, che da qui passa ad
 * «Accesso a tutti i file». Niente fuori da Android, e niente con l'accesso
 * pieno.
 */
export function PermessoMusica() {
  const accesso = useAccessoTelefono();
  const daChiedere = cosaChiedere(accesso.stato, "impostazioni");
  if (daChiedere === "niente") return null;
  return (
    <>
      <p className="nota">
        {t(daChiedere === "musica" ? "telefono.accesso.manca" : "telefono.accesso.soloMedia")}
      </p>
      <div className="azioni">
        <button
          type="button"
          className="bottone"
          onClick={accesso.consenti}
          disabled={accesso.inCorso}
        >
          {t("telefono.accesso.consenti")}
        </button>
      </div>
    </>
  );
}

export function AccessoTelefono({
  accesso,
  onSalta,
}: {
  accesso: Accesso;
  onSalta: () => void;
}) {
  if (accesso.stato === "verifica") {
    return (
      <p className="primo-attesa" role="status">
        {t("primo.cercando")}
      </p>
    );
  }
  return (
    <>
      <h2 className="primo-sotto">{t("telefono.accesso.title")}</h2>
      <p>{t("telefono.accesso.body")}</p>
      <p className="primo-avviso">{t("telefono.accesso.livelli")}</p>
      <div className="primo-tasti">
        <button
          type="button"
          className="bottone primario btn-accent"
          onClick={accesso.consenti}
          disabled={accesso.inCorso}
        >
          {t("telefono.accesso.consenti")}
        </button>
        {accesso.stato === "solo-media" && (
          <button type="button" className="bottone" onClick={accesso.soloBrani}>
            {t("telefono.accesso.soloBrani")}
          </button>
        )}
        <button type="button" className="bottone minuto" onClick={onSalta}>
          {t("primo.salta")}
        </button>
      </div>
    </>
  );
}
