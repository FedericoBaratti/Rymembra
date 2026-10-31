/**
 * Il generatore di tavolozze: una tinta, e il tema intero che ne viene.
 *
 * Da un colore scelto, o dalla copertina del brano in ascolto. Il nucleo
 * (`dinamico::tavolozza_da`) costruisce superfici, testi, accento e stati in
 * OKLCH e sposta la chiarezza di ogni colore finché ogni coppia che il
 * validatore misura regge il contrasto: quel che si applica di qui non accende
 * nessun avviso. Si guarda prima, si applica dopo, e l'applicazione è un passo
 * solo della storia.
 */
import { useState } from "react";

import { ipc } from "../ipc";
import { t } from "../lingue";
import { Icona } from "../parti/Icone";
import { useStudio } from "./contesto";
import { operazioniTavolozza, propostaPer, type PropostaTavolozza } from "./tavolozza";

export function Generatore() {
  const { s, documento, scriviMolti, onErrore } = useStudio();
  const [tinta, setTinta] = useState("#8b7cf6");
  const [chiesta, setChiesta] = useState<PropostaTavolozza | null>(null);
  const scuraDiBase = documento?.["scheme"] !== "light";
  // Il tema che si sta guardando è quello che si genera: la variante di una
  // skin scura è chiara, e viceversa.
  const scura = s.tema === "base" ? scuraDiBase : !scuraDiBase;
  // Solo se è del tema che si guarda: vedi `tavolozza.ts`.
  const proposta = propostaPer(chiesta, s.tema, scura);

  const genera = async (da: { colore: string } | { copertina: string }) => {
    const tema = s.tema;
    try {
      const voci = await ipc.studioTavolozzaDa(da, scura);
      setChiesta({ voci, tema, scura, daCopertina: "copertina" in da });
    } catch (e) {
      onErrore(e);
    }
  };

  const dallaCopertina = async () => {
    try {
      const stato = await ipc.riproduzioneStato();
      const impronta = stato.brano?.coverArtHash ?? null;
      if (impronta === null) {
        onErrore(new Error(t("studio3.gen.noCover")));
        return;
      }
      await genera({ copertina: impronta });
    } catch (e) {
      onErrore(e);
    }
  };

  const applica = () => {
    if (proposta === null) return;
    scriviMolti(
      operazioniTavolozza(proposta),
      t("studio3.step.generated", { tema: proposta.scura ? t("studio3.bar.dark") : t("studio3.bar.light") }),
    );
    setChiesta(null);
  };

  return (
    <div className="generatore">
      <span className="titolino">{t("studio3.gen.title", { tema: scura ? t("studio3.bar.dark") : t("studio3.bar.light") })}</span>
      <div className="fonti-generatore">
        <label className="colore-meta" title={t("studio3.gen.pick")}>
          <input
            type="color"
            aria-label={t("studio3.gen.pick")}
            value={tinta}
            onChange={(e) => setTinta(e.target.value)}
          />
        </label>
        <button type="button" className="pillola btn-ghost minuto" onClick={() => void genera({ colore: tinta })}>
          <Icona nome="i-wand" dim={13} />
          {t("studio3.gen.fromColor")}
        </button>
        <button type="button" className="pillola btn-ghost minuto" onClick={() => void dallaCopertina()}>
          <Icona nome="i-album" dim={13} />
          {t("studio3.gen.fromCover")}
        </button>
      </div>
      {proposta !== null && (
        <>
          <div className="proposta-generatore" role="list" aria-label={t("studio3.gen.preview")}>
            {proposta.voci.map((v) => (
              <span
                key={v.token}
                role="listitem"
                className="campione"
                title={`${v.token}: ${v.valore}`}
                style={{ background: v.valore }}
              />
            ))}
          </div>
          <p className="nota-variabili">
            {proposta.daCopertina ? t("studio3.gen.noteCover") : t("studio3.gen.note")}
          </p>
          <div className="azioni-variabili">
            <button type="button" className="bottone minuto primario" onClick={applica}>
              {t("studio3.gen.apply", { n: proposta.voci.length })}
            </button>
            <button type="button" className="bottone minuto" onClick={() => setChiesta(null)}>
              {t("common.cancel")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
