/**
 * Il profilo: portare le proprie impostazioni su un altro computer.
 *
 * # Perché il piano, anche qui
 *
 * Importare un profilo sovrascrive delle scelte che qualcuno ha fatto a mano —
 * la skin, il volume, le cartelle sorvegliate — e non c'è nessun annullamento
 * dopo. È la stessa famiglia di `Ripristino.tsx` e `Importa.tsx`: prima si
 * **legge** cosa cambierebbe, poi si conferma. `profilo_piano` è la stessa
 * funzione di `profilo_importa` dentro una transazione che viene abbandonata,
 * quindi l'elenco che si legge qui non è una previsione: è il risultato.
 *
 * # Perché l'elenco delle chiavi lasciate si mostra
 *
 * Il profilo porta un elenco di **inclusioni**: `nuvola.dispositivo` e la coda
 * di riproduzione non devono viaggiare, perché due computer con lo stesso
 * identificativo di dispositivo si rovinano il backup a vicenda, e gli
 * identificativi delle righe di `tracks` su un'altra libreria nominano canzoni
 * diverse. Il rovescio di un elenco di inclusioni è che dimenticarsi una chiave
 * è silenzioso — quindi non lo è: l'esportazione dice quali chiavi ha lasciato
 * indietro, e chi ne riconosce una che invece voleva può dirlo.
 */
import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";

import type { PianoProfilo } from "../ipc";
import { ipc } from "../ipc";
import { Icona } from "./Icone";

/** Come si legge una data del profilo. */
function quandoScritto(ms: number): string {
  if (ms <= 0) return "in un momento imprecisato";
  return new Date(ms).toLocaleString();
}

/** Un valore, accorciato quanto basta a stare su una riga. */
function breve(valore: string): string {
  return valore.length > 60 ? `${valore.slice(0, 57)}…` : valore;
}

export function Profilo({
  onErrore,
  onNotizia,
  onImportato,
}: {
  onErrore: (e: unknown) => void;
  onNotizia: (testo: string) => void;
  /** Il profilo è stato applicato: quel che sta in `App` va riletto. */
  onImportato: () => void;
}) {
  /** Il piano da confermare, col percorso da cui è venuto. */
  const [daApplicare, setDaApplicare] = useState<{
    percorso: string;
    piano: PianoProfilo;
  } | null>(null);
  const [inVolo, setInVolo] = useState(false);
  /** Le chiavi che l'ultima esportazione ha lasciato qui. */
  const [lasciate, setLasciate] = useState<string[]>([]);

  const esporta = async () => {
    try {
      const scelta = await save({
        defaultPath: "aether-profilo.json",
        filters: [{ name: "Profilo di Aether", extensions: ["json"] }],
      });
      if (typeof scelta !== "string") return;
      setInVolo(true);
      const esito = await ipc.profiloEsporta(scelta);
      setLasciate(esito.lasciate);
      onNotizia(
        `${esito.voci} ${esito.voci === 1 ? "impostazione scritta" : "impostazioni scritte"} in ${scelta}`,
      );
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
    }
  };

  const leggi = async () => {
    try {
      const scelta = await open({
        multiple: false,
        filters: [{ name: "Profilo di Aether", extensions: ["json"] }],
      });
      if (typeof scelta !== "string") return;
      setInVolo(true);
      setDaApplicare({ percorso: scelta, piano: await ipc.profiloPiano(scelta) });
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
    }
  };

  const applica = async () => {
    if (!daApplicare) return;
    try {
      setInVolo(true);
      const fatto = await ipc.profiloImporta(daApplicare.percorso);
      setDaApplicare(null);
      onNotizia(
        fatto.cambi.length === 0
          ? "Il profilo non ha cambiato niente: era già tutto così."
          : `${fatto.cambi.length} ${fatto.cambi.length === 1 ? "impostazione applicata" : "impostazioni applicate"}`,
      );
      onImportato();
    } catch (e) {
      onErrore(e);
    } finally {
      setInVolo(false);
    }
  };

  return (
    <>
      <p className="nota">
        Un file solo con le tue scelte — tema, skin, equalizzatore, scorciatoie,
        cartelle — da riaprire su un altro computer o dopo una
        reinstallazione. <strong>Non</strong> contiene la libreria, né i token
        dei servizi collegati: quelli stanno nel portachiavi di sistema e da lì
        non escono.
      </p>

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo}
          onClick={() => void esporta()}
        >
          <Icona nome="i-import" dim={15} />
          Esporta il profilo…
        </button>
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo}
          onClick={() => void leggi()}
        >
          <Icona nome="i-import" dim={15} />
          Leggi un profilo…
        </button>
      </div>

      {lasciate.length > 0 && (
        <p className="nota">
          Rimaste qui: {lasciate.map(breve).join(", ")}. Sono chiavi che
          identificano <em>questo</em> computer o che nominano righe di{" "}
          <em>questa</em> libreria — su un&apos;altra macchina direbbero cose
          sbagliate invece di niente.
        </p>
      )}

      {daApplicare && (
        <div className="anteprima-spotify">
          <h3 className="titoletto">
            Profilo scritto {quandoScritto(daApplicare.piano.creatoMs)}
          </h3>

          {daApplicare.piano.cambi.length === 0 ? (
            <p className="niente empty-state">
              Non cambierebbe niente: le impostazioni di questo computer sono già
              quelle del file.
            </p>
          ) : (
            <ul className="cartelle">
              {daApplicare.piano.cambi.map((c) => (
                <li className="cartella" key={c.chiave}>
                  <span className="percorso" title={`${c.prima ?? "(assente)"} → ${c.dopo}`}>
                    <code>{c.chiave}</code>: {c.prima === null ? "—" : breve(c.prima)} →{" "}
                    {breve(c.dopo)}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {daApplicare.piano.invariate > 0 && (
            <p className="nota">
              {daApplicare.piano.invariate}{" "}
              {daApplicare.piano.invariate === 1
                ? "impostazione è già uguale"
                : "impostazioni sono già uguali"}
              .
            </p>
          )}

          {daApplicare.piano.percorsiMancanti.length > 0 && (
            <p className="nota">
              <strong>Questi percorsi non esistono su questo computer</strong>:{" "}
              {daApplicare.piano.percorsiMancanti.map(breve).join(", ")}. Si
              scrivono lo stesso — una cartella può essere su un disco staccato
              adesso e attaccato domani — ma la scansione non troverà niente
              finché non ci sono.
            </p>
          )}

          {daApplicare.piano.sconosciute.length > 0 && (
            <p className="nota">
              Chiavi che questa versione non porta, e che verranno ignorate:{" "}
              {daApplicare.piano.sconosciute.map(breve).join(", ")}.
            </p>
          )}

          <p className="nota">
            Tema, skin e scorciatoie si vedono subito. Il volume,
            l&apos;equalizzatore e la normalizzazione li legge il motore quando
            si apre: quelli cambiano alla prossima apertura.
          </p>

          <div className="azioni">
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={inVolo || daApplicare.piano.cambi.length === 0}
              onClick={() => void applica()}
            >
              {inVolo ? "Applico…" : "Applica"}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => setDaApplicare(null)}
            >
              Lascia stare
            </button>
          </div>
        </div>
      )}
    </>
  );
}
