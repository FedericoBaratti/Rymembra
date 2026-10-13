/**
 * Il centro dello Studio: la tela, e i comandi per guardarla.
 *
 * Sopra, la scena — quale pagina, e che cosa le sta sopra — con lo zoom, la
 * sonda e «rigioca». Sotto, quando una parte è scelta, la barra degli stati:
 * scegliere «al passaggio» lì è la stessa scelta dell'ispettore, e la tela lo
 * forza su ogni occorrenza della parte, così si disegna l'hover senza doverci
 * tenere sopra il puntatore. In fondo i numeri che riguardano tutta la skin:
 * errori, avvisi, contrasti, movimento della finestra.
 *
 * La tela resta all'ultimo foglio **valido**: mentre il testo è rotto — una
 * virgola a metà nel codice — si vede l'ultima versione buona e una fascia
 * dice perché, invece di una finestra bianca.
 */
import { useDeferredValue, useEffect, useRef, useState } from "react";

import { t, tSe } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { useStudio } from "./contesto";
import { parteScelta, testo, type Zoom } from "./negozio";
import { PAGINE, SOVRAPPOSIZIONI, type Pagina, type Sovrapposizione } from "./scena";
import { Tela } from "./Tela";

export function Centro() {
  const { s, manda, parti, documento } = useStudio();
  const valido = s.valido;
  // La tela resta indietro di un disegno mentre si trascina un cursore: i
  // controlli rispondono subito, e la pagina finta si ridisegna quando può.
  // Il foglio e il suo id restano indietro **insieme**: sono la stessa
  // validazione, e un foglio nuovo con l'id vecchio (o il contrario) sarebbe
  // una tela senza regole per un fotogramma.
  const foglio = useDeferredValue(valido);
  const css = foglio?.css ?? "";
  const radice = foglio === null || foglio.radice === "" ? s.id : foglio.radice;
  const scuraDiBase = (documento?.["scheme"] ?? "dark") !== "light";
  const scura = s.tema === "base" ? scuraDiBase : !scuraDiBase;
  const scelta = parteScelta(s);
  const layout = valido?.layout ?? null;
  const rotto =
    s.esito !== null && s.esito.errori.length > 0 && s.validato === testo(s) ? s.esito : null;

  // Una bozza che non si è mai compilata non ha niente da mostrare sulla tela:
  // i Problemi si aprono da sé, una volta, perché lì c'è la strada.
  const maiValida = valido === null && rotto !== null;
  const aperti = useRef(false);
  useEffect(() => {
    if (!maiValida || aperti.current) return;
    aperti.current = true;
    manda({ tipo: "imposta", campi: { dock: "problemi" } });
  }, [maiValida, manda]);

  return (
    <div className="tavolo-studio">
      <BarraScena />
      {scelta !== null && <BarraStati parte={scelta} />}
      {rotto !== null && (
        <div className="fascia-rotta" role="status">
          <Icona nome="i-alert" dim={14} />
          <span>{t("studio3.canvas.broken", { n: rotto.errori.length })}</span>
          <button
            type="button"
            className="bottone minuto"
            onClick={() => manda({ tipo: "imposta", campi: { dock: "problemi" } })}
          >
            {t("studio3.canvas.showProblems")}
          </button>
        </div>
      )}
      {valido === null ? (
        <div className="tela-attesa" role="status">
          {s.esito === null ? t("studio3.canvas.waiting") : t("studio3.canvas.neverValid")}
        </div>
      ) : (
        <Tela
          css={css}
          // L'id sotto cui il foglio è compilato, non quello del negozio: i
          // due viaggiano insieme, e la tela non resta mai con un foglio i cui
          // selettori cercano un altro `data-skin`.
          id={radice}
          tema={scura ? "dark" : "light"}
          densita={layout?.density ?? "comfortable"}
          movimento={layout?.motion ?? "full"}
          shell={layout?.shell ?? null}
          scena={s.scena}
          larghezza={s.larghezza}
          zoom={s.zoom}
          inPausa={s.inPausa}
          parti={parti}
          scelta={scelta}
          statoForzato={s.forza ? s.stato : null}
          sonda={s.sonda}
          rigioca={s.rigioca}
          onScegli={(nome) => manda({ tipo: "scegli", selezione: { tipo: "parte", nome } })}
        />
      )}
      <PiedeTela />
    </div>
  );
}

/** La pagina, le sovrapposizioni, lo zoom, la sonda e «rigioca». */
function BarraScena() {
  const { s, manda } = useStudio();
  const [sopraAperto, setSopraAperto] = useState(false);
  const sopra = useRef<HTMLDivElement>(null);
  const accese = new Set(s.scena.sovrapposizioni);

  // L'elenco delle sovrapposizioni si chiude cliccando altrove, come un menù.
  useEffect(() => {
    if (!sopraAperto) return;
    const fuori = (e: PointerEvent) => {
      if (!sopra.current?.contains(e.target as Node)) setSopraAperto(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setSopraAperto(false);
      }
    };
    document.addEventListener("pointerdown", fuori);
    document.addEventListener("keydown", esc, true);
    return () => {
      document.removeEventListener("pointerdown", fuori);
      document.removeEventListener("keydown", esc, true);
    };
  }, [sopraAperto]);

  const alterna = (quale: Sovrapposizione) => {
    const nuove = accese.has(quale)
      ? s.scena.sovrapposizioni.filter((q) => q !== quale)
      : [...s.scena.sovrapposizioni, quale];
    manda({ tipo: "scena", scena: { ...s.scena, sovrapposizioni: nuove } });
  };

  return (
    <div className="barra-scena">
      <label className="scelta-pagina">
        <span>{t("studio3.scene.page")}</span>
        <select
          className="field-input"
          value={s.scena.pagina}
          onChange={(e) =>
            manda({ tipo: "scena", scena: { ...s.scena, pagina: e.target.value as Pagina } })
          }
        >
          {PAGINE.map((p) => (
            <option key={p} value={p}>
              {t(`studio3.scene.page.${p}`)}
            </option>
          ))}
        </select>
      </label>

      <div className="scelta-sopra" ref={sopra}>
        <button
          type="button"
          className="bottone minuto"
          aria-expanded={sopraAperto}
          aria-haspopup="true"
          onClick={() => setSopraAperto((a) => !a)}
        >
          <Icona nome="i-layers" dim={14} />
          <span>
            {accese.size === 0
              ? t("studio3.scene.over.none")
              : t("studio3.scene.over.some", { n: accese.size })}
          </span>
          <Icona nome="i-chev-d" dim={12} />
        </button>
        {sopraAperto && (
          <div className="elenco-sopra" role="group" aria-label={t("studio3.scene.over")}>
            {SOVRAPPOSIZIONI.map((q) => (
              <label key={q} className="voce-sopra">
                <input type="checkbox" checked={accese.has(q)} onChange={() => alterna(q)} />
                <span>{t(`studio3.scene.over.${q}`)}</span>
              </label>
            ))}
          </div>
        )}
      </div>

      <span className="spinta" />

      <Segmentato<"adatta" | "1" | "2">
        etichetta={t("studio3.scene.zoom")}
        classe="minuto"
        scelta={String(s.zoom) as "adatta" | "1" | "2"}
        onScegli={(z) =>
          manda({ tipo: "imposta", campi: { zoom: (z === "adatta" ? z : Number(z)) as Zoom } })
        }
        voci={[
          { chiave: "adatta", etichetta: t("studio3.scene.zoom.fit") },
          { chiave: "1", etichetta: "100%" },
          { chiave: "2", etichetta: "200%" },
        ]}
      />
      <button
        type="button"
        className="tasto-icona icon-btn"
        aria-pressed={s.sonda}
        aria-label={t("studio3.scene.probe")}
        title={t("studio3.scene.probe")}
        onClick={() => manda({ tipo: "imposta", campi: { sonda: !s.sonda } })}
      >
        <Icona nome="i-probe" dim={15} />
      </button>
      <button
        type="button"
        className="tasto-icona icon-btn"
        aria-label={t("studio3.scene.replay")}
        title={t("studio3.scene.replay")}
        onClick={() => manda({ tipo: "rigioca" })}
      >
        <Icona nome="i-repeat" dim={15} />
      </button>
    </div>
  );
}

/**
 * Gli stati della parte scelta: quale si modifica, e se la tela lo forza.
 *
 * Il puntino dice quali la skin dichiara già — nella base o nella variante,
 * secondo il tema che si guarda — così si vede a colpo d'occhio che cosa la
 * parte fa al passaggio e che cosa no.
 */
function BarraStati({ parte }: { parte: string }) {
  const { s, manda, registro, documento } = useStudio();
  const radice =
    s.tema === "variante"
      ? (((documento?.["variant"] ?? {}) as Record<string, unknown>)["parts"] ?? {})
      : (documento?.["parts"] ?? {});
  const dichiarata = ((radice as Record<string, unknown>)[parte] ?? {}) as Record<string, unknown>;
  const stati = (dichiarata["states"] ?? {}) as Record<string, unknown>;

  return (
    <div className="barra-stati" role="toolbar" aria-label={t("studio3.states.aria", { parte })}>
      <code className="parte-stati">.{parte}</code>
      <div className="chip-stati" role="radiogroup" aria-label={t("studio3.states.which")}>
        {[null, ...registro.stati.map((st) => st.name)].map((nome) => (
          <button
            key={nome ?? "base"}
            type="button"
            role="radio"
            className="chip-stato"
            aria-checked={s.stato === nome}
            data-dichiarato={(nome !== null && stati[nome] !== undefined) || undefined}
            onClick={() => manda({ tipo: "stato", stato: nome })}
          >
            {nome === null ? t("studio3.state.base") : nomeStato(nome)}
          </button>
        ))}
      </div>
      <label className="forza-stato" title={t("studio3.states.force.hint")}>
        <input
          type="checkbox"
          checked={s.forza}
          onChange={(e) => manda({ tipo: "imposta", campi: { forza: e.target.checked } })}
        />
        <span>{t("studio3.states.force")}</span>
      </label>
    </div>
  );
}

/** I numeri di tutta la skin, in fondo alla tela. */
function PiedeTela() {
  const { s, manda, registro } = useStudio();
  const esito = s.esito;
  if (esito === null) return <footer className="piede-tela" />;
  const errori = esito.errori.length;
  const avvisi = esito.avvisi.length;
  const bassi = esito.contrasti.filter((c) => !c.passa).length;
  const finestra = esito.costoFinestra;
  const oltre = finestra > registro.windowMotionBudget;
  const apri = () => manda({ tipo: "imposta", campi: { dock: "problemi" } });

  return (
    <footer className="piede-tela">
      <button
        type="button"
        className="voce-piede"
        data-esito={errori > 0 ? "male" : "bene"}
        onClick={apri}
      >
        <Icona nome={errori > 0 ? "i-alert" : "i-check"} dim={13} />
        {errori > 0 ? t("studio3.foot.errors", { n: errori }) : t("studio3.foot.valid")}
      </button>
      <button
        type="button"
        className="voce-piede"
        data-esito={avvisi > 0 ? "attenzione" : "bene"}
        onClick={apri}
      >
        {t("studio3.foot.warnings", { n: avvisi })}
      </button>
      <button
        type="button"
        className="voce-piede"
        data-esito={bassi > 0 ? "attenzione" : "bene"}
        onClick={apri}
      >
        {t("studio3.foot.contrast", { n: bassi })}
      </button>
      <span className="spinta" />
      <span
        className="voce-piede"
        data-esito={oltre ? "attenzione" : "bene"}
        title={t("studio3.foot.motion.hint")}
      >
        <Icona nome="i-timeline" dim={13} />
        {t("studio3.foot.motion", { n: finestra, di: registro.windowMotionBudget })}
      </span>
      <span className="voce-piede mono">{t("studio3.foot.compiled", { ms: esito.compilatoMs })}</span>
    </footer>
  );
}

/** Il nome di uno stato del DOM, per chi legge: «al passaggio», «in ascolto». */
export function nomeStato(nome: string): string {
  return tSe(`studio3.state.${nome}`, nome);
}
