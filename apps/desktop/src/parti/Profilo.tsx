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
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import { dataOra } from "../formato";

/** Come si legge una data del profilo. */
function quandoScritto(ms: number): string {
  return ms <= 0 ? t("profile.unknownDate") : dataOra(ms);
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
        filters: [{ name: t("profile.file"), extensions: ["json"] }],
      });
      if (typeof scelta !== "string") return;
      setInVolo(true);
      const esito = await ipc.profiloEsporta(scelta);
      setLasciate(esito.lasciate);
      onNotizia(t("profile.exported", { n: esito.voci, dove: scelta }));
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
        filters: [{ name: t("profile.file"), extensions: ["json"] }],
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
          ? t("profile.nothingChanged")
          : t("profile.applied", { n: fatto.cambi.length }),
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
        <Trans
          k="profile.note"
          v={{ non: <strong>{t("profile.note.not")}</strong> }}
        />
      </p>

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo}
          onClick={() => void esporta()}
        >
          <Icona nome="i-import" dim={15} />
          {t("profile.export")}
        </button>
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inVolo}
          onClick={() => void leggi()}
        >
          <Icona nome="i-import" dim={15} />
          {t("profile.read")}
        </button>
      </div>

      {lasciate.length > 0 && (
        <p className="nota">
          <Trans
            k="profile.leftHere"
            n={{ chiavi: lasciate.map(breve).join(", ") }}
            v={{
              questo: <em>{t("profile.leftHere.this")}</em>,
              questa: <em>{t("profile.leftHere.thisLib")}</em>,
            }}
          />
        </p>
      )}

      {daApplicare && (
        <div className="scheda-anteprima">
          <h3 className="titoletto">
            {t("profile.writtenOn", {
              quando: quandoScritto(daApplicare.piano.creatoMs),
            })}
          </h3>

          {daApplicare.piano.cambi.length === 0 ? (
            <p className="niente empty-state">{t("profile.noChanges")}</p>
          ) : (
            <ul className="cartelle">
              {daApplicare.piano.cambi.map((c) => (
                <li className="cartella" key={c.chiave}>
                  <span
                    className="percorso"
                    title={`${c.prima ?? t("profile.absent")} → ${c.dopo}`}
                  >
                    <code>{c.chiave}</code>:{" "}
                    {c.prima === null ? "—" : breve(c.prima)} → {breve(c.dopo)}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {daApplicare.piano.invariate > 0 && (
            <p className="nota">
              {t("profile.same", { n: daApplicare.piano.invariate })}
            </p>
          )}

          {daApplicare.piano.percorsiMancanti.length > 0 && (
            <p className="nota">
              <Trans
                k="profile.missingPaths"
                n={{
                  percorsi: daApplicare.piano.percorsiMancanti
                    .map(breve)
                    .join(", "),
                }}
                v={{
                  titolo: <strong>{t("profile.missingPaths.title")}</strong>,
                }}
              />
            </p>
          )}

          {daApplicare.piano.sconosciute.length > 0 && (
            <p className="nota">
              {t("profile.unknownKeys", {
                chiavi: daApplicare.piano.sconosciute.map(breve).join(", "),
              })}
            </p>
          )}

          <p className="nota">{t("profile.whenApplied")}</p>

          <div className="azioni">
            <button
              type="button"
              className="bottone primario btn-accent"
              disabled={inVolo || daApplicare.piano.cambi.length === 0}
              onClick={() => void applica()}
            >
              {inVolo ? t("profile.applying") : t("profile.apply")}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => setDaApplicare(null)}
            >
              {t("profile.leaveIt")}
            </button>
          </div>
        </div>
      )}
    </>
  );
}
