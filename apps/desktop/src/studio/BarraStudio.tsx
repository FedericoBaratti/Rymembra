/**
 * La barra in cima allo Studio: chi si sta disegnando, e che cosa farne.
 *
 * A sinistra la skin — nome, versione, com'è la bozza — e i due tasti della
 * storia, che dicono **che cosa** annullano: «Annulla: raggio di card» vale più
 * di una freccia, perché dopo dieci minuti di cursori nessuno ricorda qual era
 * l'ultimo. In mezzo come la si guarda: il tema, la larghezza della finestra
 * finta, il disco che suona o no. A destra quel che se ne fa: provarla sulla
 * finestra vera, usarla, esportarla.
 *
 * Lo spazio vuoto della barra è la fascia del titolo: da lì si trascina la
 * finestra, come dalla testata dell'app (`BarraTitolo.tsx` ascolta sul
 * documento, e salta tutto quel che è un bottone o un campo).
 */
import { t } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { useStudio } from "./contesto";
import { inValidazione, statoBozza } from "./negozio";
import type { Larghezza } from "./scena";
import { cosaAnnulla, cosaRipete } from "./storia";

export function BarraStudio({
  onEsci,
  onSalvaEUsa,
  onEsporta,
  onProva,
}: {
  onEsci: () => void;
  onSalvaEUsa: () => void;
  onEsporta: () => void;
  onProva: () => void;
}) {
  const { s, manda, documento } = useStudio();
  const meta = (documento?.["meta"] ?? {}) as Record<string, unknown>;
  const nome = typeof meta["name"] === "string" && meta["name"] !== "" ? meta["name"] : s.id;
  const versione = typeof meta["version"] === "string" ? meta["version"] : null;
  const scuraDiBase = documento?.["scheme"] !== "light";
  const bozza = statoBozza(s);
  const annulla = cosaAnnulla(s.storia);
  const ripete = cosaRipete(s.storia);
  const rotta = bozza === "rotta";

  return (
    <header className="barra-studio">
      <button
        type="button"
        className="tasto-icona icon-btn"
        aria-label={t("studio3.bar.exit")}
        title={t("studio3.bar.exit")}
        onClick={onEsci}
      >
        <Icona nome="i-chev-l" dim={16} />
      </button>
      <div className="chi-studio">
        <button
          type="button"
          className="nome-skin"
          title={t("studio3.bar.document")}
          aria-pressed={s.selezione?.tipo === "documento"}
          onClick={() => manda({ tipo: "scegli", selezione: { tipo: "documento" } })}
        >
          <Icona nome="i-skin" dim={16} />
          <strong>{nome}</strong>
          {versione !== null && <span className="versione-skin">v{versione}</span>}
        </button>
        <code className="id-skin">{s.id}</code>
        <span className="stato-bozza" data-stato={bozza} role="status">
          {inValidazione(s) && bozza !== "rotta"
            ? t("studio3.bar.checking")
            : t(`studio3.bar.state.${bozza}`)}
        </span>
      </div>

      <div className="storia-studio" role="group" aria-label={t("studio3.bar.history")}>
        <button
          type="button"
          className="tasto-icona icon-btn"
          disabled={annulla === null}
          aria-label={
            annulla === null ? t("studio3.bar.undo") : t("studio3.bar.undoWhat", { cosa: annulla })
          }
          title={
            annulla === null ? t("studio3.bar.undo") : t("studio3.bar.undoWhat", { cosa: annulla })
          }
          onClick={() => manda({ tipo: "annulla" })}
        >
          <Icona nome="i-undo" dim={16} />
        </button>
        <button
          type="button"
          className="tasto-icona icon-btn"
          disabled={ripete === null}
          aria-label={
            ripete === null ? t("studio3.bar.redo") : t("studio3.bar.redoWhat", { cosa: ripete })
          }
          title={
            ripete === null ? t("studio3.bar.redo") : t("studio3.bar.redoWhat", { cosa: ripete })
          }
          onClick={() => manda({ tipo: "ripeti" })}
        >
          <Icona nome="i-redo" dim={16} />
        </button>
      </div>

      <span className="spinta" />

      <div className="vista-studio">
        <Segmentato
          etichetta={t("studio3.bar.theme")}
          classe="minuto"
          scelta={s.tema}
          onScegli={(tema) => manda({ tipo: "imposta", campi: { tema } })}
          voci={[
            {
              chiave: "base",
              etichetta: scuraDiBase ? t("studio3.bar.dark") : t("studio3.bar.light"),
            },
            {
              chiave: "variante",
              etichetta: scuraDiBase ? t("studio3.bar.light") : t("studio3.bar.dark"),
            },
          ]}
        />
        <Segmentato<Larghezza>
          etichetta={t("studio3.bar.width")}
          classe="minuto"
          scelta={s.larghezza}
          onScegli={(larghezza) => manda({ tipo: "imposta", campi: { larghezza } })}
          voci={[
            { chiave: "tre", etichetta: t("studio3.bar.width.tre") },
            { chiave: "due", etichetta: t("studio3.bar.width.due") },
            { chiave: "stretta", etichetta: t("studio3.bar.width.stretta") },
          ]}
        />
        <button
          type="button"
          className="tasto-icona icon-btn"
          aria-pressed={!s.inPausa}
          aria-label={s.inPausa ? t("studio3.bar.play") : t("studio3.bar.pause")}
          title={s.inPausa ? t("studio3.bar.play") : t("studio3.bar.pause")}
          onClick={() => manda({ tipo: "imposta", campi: { inPausa: !s.inPausa } })}
        >
          <Icona nome={s.inPausa ? "i-play" : "i-pause"} dim={15} />
        </button>
      </div>

      <span className="spinta" />

      <div className="azioni-studio">
        <button
          type="button"
          className="tasto-icona icon-btn"
          aria-pressed={s.chat}
          aria-label={t("studio3.bar.chat")}
          title={t("studio3.bar.chat")}
          onClick={() => manda({ tipo: "imposta", campi: { chat: !s.chat } })}
        >
          <Icona nome="i-ia" dim={16} />
        </button>
        <button
          type="button"
          className="bottone minuto"
          disabled={s.valido === null || rotta}
          title={rotta ? t("studio3.bar.broken") : t("studio3.bar.try.hint")}
          onClick={onProva}
        >
          <Icona nome="i-eye" dim={14} />
          <span>{t("studio3.bar.try")}</span>
        </button>
        <button
          type="button"
          className="bottone minuto"
          disabled={rotta}
          title={rotta ? t("studio3.bar.broken") : undefined}
          onClick={onEsporta}
        >
          {t("studio3.bar.export")}
        </button>
        <button
          type="button"
          className="bottone minuto primario"
          disabled={rotta}
          title={rotta ? t("studio3.bar.broken") : t("studio3.bar.use.hint")}
          onClick={onSalvaEUsa}
        >
          {t("studio3.bar.use")}
        </button>
      </div>
    </header>
  );
}
