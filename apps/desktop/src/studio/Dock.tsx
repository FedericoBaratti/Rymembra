/**
 * Il cassetto in basso: la timeline, i problemi, il codice.
 *
 * Si apre dove serve — «apri nella timeline» da un'animazione, il conto degli
 * errori in fondo alla tela, «vai alla riga» da un problema — e si chiude con
 * la sua ×. L'altezza si trascina dal bordo di sopra.
 */
import { useEffect, useMemo, useState, type PointerEvent as EventoPuntatore } from "react";

import type { Avviso, Problema } from "../ipc";
import { numero } from "../formato";
import { t, tSe } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { useStudio } from "./contesto";
import { differenze, differenzePerPercorso, quanteCambiano } from "./differenze";
import { EditorCodice, useFonti, useSegni } from "./EditorCodice";
import { inValidazione, testo, type SchedaDock } from "./negozio";
import { leggi, passiDi, rinominaChiave, scrivi as stampa, valoreIn } from "./patch";
import { Schede } from "./Schede";
import { Timeline } from "./Timeline";
import { useVai } from "./vai";

/** L'altezza del cassetto: fra questi due capi. */
const ALTEZZA_MIN = 140;
const ALTEZZA_MAX = 640;

export function Dock() {
  const { s, manda } = useStudio();
  const [altezza, setAltezza] = useState(260);
  const scheda: SchedaDock = s.dock ?? "problemi";
  const errori = s.esito?.errori.length ?? 0;
  const avvisi = s.esito?.avvisi.length ?? 0;

  const trascina = (e: EventoPuntatore<HTMLDivElement>) => {
    const da = e.clientY;
    const prima = altezza;
    const bersaglio = e.currentTarget;
    bersaglio.setPointerCapture(e.pointerId);
    const muovi = (m: PointerEvent) =>
      setAltezza(Math.max(ALTEZZA_MIN, Math.min(ALTEZZA_MAX, prima + da - m.clientY)));
    const lascia = () => {
      bersaglio.removeEventListener("pointermove", muovi);
      bersaglio.removeEventListener("pointerup", lascia);
      bersaglio.removeEventListener("pointercancel", lascia);
    };
    bersaglio.addEventListener("pointermove", muovi);
    bersaglio.addEventListener("pointerup", lascia);
    bersaglio.addEventListener("pointercancel", lascia);
  };

  return (
    <section className="dock-studio" aria-label={t("studio3.dock.aria")} style={{ height: altezza }}>
      <div
        className="maniglia-dock"
        role="separator"
        aria-orientation="horizontal"
        aria-label={t("studio3.dock.resize")}
        aria-valuemin={ALTEZZA_MIN}
        aria-valuemax={ALTEZZA_MAX}
        aria-valuenow={altezza}
        tabIndex={0}
        onPointerDown={trascina}
        onKeyDown={(e) => {
          if (e.key === "ArrowUp") setAltezza((a) => Math.min(ALTEZZA_MAX, a + 20));
          if (e.key === "ArrowDown") setAltezza((a) => Math.max(ALTEZZA_MIN, a - 20));
        }}
      />
      <div className="testa-dock">
        <Schede<SchedaDock>
          etichetta={t("studio3.dock.aria")}
          scelta={scheda}
          onScegli={(dock) => manda({ tipo: "imposta", campi: { dock } })}
          voci={[
            ["timeline", t("studio3.dock.timeline")],
            ["problemi", t("studio3.dock.problems"), errori + avvisi],
            ["codice", t("studio3.dock.code")],
          ]}
          coda={
            <button
              type="button"
              className="tasto-icona icon-btn chiudi-dock"
              aria-label={t("studio3.dock.close")}
              title={t("studio3.dock.close")}
              onClick={() => manda({ tipo: "imposta", campi: { dock: null } })}
            >
              <Icona nome="i-x" dim={13} />
            </button>
          }
        />
      </div>
      <div className="corpo-dock" role="tabpanel">
        {scheda === "timeline" && <Timeline />}
        {scheda === "problemi" && <Problemi />}
        {scheda === "codice" && <Codice />}
      </div>
    </section>
  );
}

// ── Problemi ─────────────────────────────────────────────────────────────────

function Problemi() {
  const { s, manda, documento, scrivi, scriviTesto } = useStudio();
  const vai = useVai();
  const esito = s.esito;
  if (esito === null) return <p className="nota-dock">{t("studio3.canvas.waiting")}</p>;
  const bassi = esito.contrasti.filter((c) => !c.passa);
  const vuoto = esito.errori.length === 0 && esito.avvisi.length === 0 && bassi.length === 0;

  /**
   * «Correggi» con un nome suggerito: se il percorso punta a un **valore**
   * scritto male — un'animazione richiamata col nome sbagliato — si riscrive
   * il valore; se punta a una **chiave** — una parte che non esiste — si
   * rinomina la chiave.
   */
  const correggi = (problema: Problema, giusto: string) => {
    if (documento === null) return;
    const passi = passiDi(documento, problema.path);
    const etichetta = t("studio3.step.fix", { da: problema.path, a: giusto });
    if (passi !== null && typeof valoreIn(documento, passi) === "string") {
      scrivi(passi, giusto, etichetta);
      return;
    }
    scriviTesto(rinominaChiave(testo(s), problema.path, giusto), etichetta, `correggi:${problema.path}`);
  };

  const riga = (p: Problema | Avviso) =>
    p.riga === null ? null : (
      <button
        type="button"
        className="pillola btn-ghost minuto"
        onClick={() => manda({ tipo: "vaiRiga", riga: p.riga ?? 1, colonna: p.colonna ?? 1 })}
      >
        {t("studio3.problems.line", { n: p.riga })}
      </button>
    );

  return (
    <div className="problemi-studio" aria-live="polite">
      {inValidazione(s) && <p className="nota-dock">{t("studio3.bar.checking")}</p>}
      {vuoto && (
        <p className="tutto-bene">
          <Icona nome="i-check" dim={14} />
          {t("studio3.problems.none")}
        </p>
      )}
      {esito.errori.length > 0 && (
        <section>
          <h4 className="titolo-dock" data-esito="male">
            {t("studio3.problems.errors", { n: esito.errori.length })}
          </h4>
          <ul className="elenco-problemi">
            {esito.errori.map((p, i) => (
              <li key={`${p.path}:${i}`} className="problema" data-esito="male">
                <Icona nome="i-alert" dim={13} />
                <div className="testo-problema">
                  <p>{p.message}</p>
                  <code>{p.path || "—"}</code>
                </div>
                <div className="azioni-problema">
                  {p.forse.slice(0, 3).map((giusto) => (
                    <button key={giusto} type="button" className="pillola minuto" onClick={() => correggi(p, giusto)}>
                      {t("studio3.problems.fix", { nome: giusto })}
                    </button>
                  ))}
                  {p.path !== "" && (
                    <button type="button" className="pillola btn-ghost minuto" onClick={() => vai(p.path)}>
                      {t("studio3.problems.go")}
                    </button>
                  )}
                  {riga(p)}
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      {esito.avvisi.length > 0 && (
        <section>
          <h4 className="titolo-dock" data-esito="attenzione">
            {t("studio3.problems.warnings", { n: esito.avvisi.length })}
          </h4>
          <ul className="elenco-problemi">
            {esito.avvisi.map((a, i) => (
              <li key={`${a.path}:${i}`} className="problema" data-esito="attenzione">
                <span className="tipo-avviso">
                  {/* `tSe`: il genere dell'avviso lo sceglie il nucleo, e uno
                      nato dopo questa finestra si legge col suo nome invece che
                      con la chiave nuda. */}
                  {tSe(`studio3.warning.${a.kind}`, a.kind)}
                </span>
                <div className="testo-problema">
                  <p>{a.message}</p>
                  <code>{a.path || "—"}</code>
                </div>
                <div className="azioni-problema">
                  {a.path !== "" && (
                    <button type="button" className="pillola btn-ghost minuto" onClick={() => vai(a.path)}>
                      {t("studio3.problems.go")}
                    </button>
                  )}
                  {riga(a)}
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      {bassi.length > 0 && (
        <section>
          <h4 className="titolo-dock" data-esito="attenzione">
            {t("studio3.problems.contrast", { n: bassi.length })}
          </h4>
          <ul className="elenco-problemi">
            {bassi.map((c) => (
              <li key={`${c.davanti}/${c.dietro}`} className="problema" data-esito="attenzione">
                <span className="campioni-contrasto" aria-hidden="true">
                  Aa
                </span>
                <div className="testo-problema">
                  <p>
                    {t("studio3.problems.contrastPair", {
                      davanti: c.davanti,
                      dietro: c.dietro,
                      base: numero(c.base, 2),
                      variante: c.variante === null ? "—" : numero(c.variante, 2),
                    })}
                  </p>
                </div>
                <div className="azioni-problema">
                  <button type="button" className="pillola btn-ghost minuto" onClick={() => vai(`tokens.${c.davanti}`)}>
                    {c.davanti}
                  </button>
                  <button type="button" className="pillola btn-ghost minuto" onClick={() => vai(`tokens.${c.dietro}`)}>
                    {c.dietro}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

// ── Codice ───────────────────────────────────────────────────────────────────

type VistaCodice = "documento" | "css" | "differenze";

function Codice() {
  const { s, manda, documento, scriviTesto } = useStudio();
  const [vista, setVista] = useState<VistaCodice>("documento");
  const adesso = testo(s);
  const fonti = useFonti([]);
  const segni = useSegni();

  // «Vai alla riga» dai Problemi: si torna al documento, e l'editor ci porta.
  useEffect(() => {
    if (s.riga !== null) setVista("documento");
  }, [s.riga]);

  return (
    <div className="codice-dock">
      <div className="barra-codice">
        <Segmentato<VistaCodice>
          etichetta={t("studio3.code.view")}
          classe="minuto"
          scelta={vista}
          onScegli={setVista}
          voci={[
            { chiave: "documento", etichetta: t("studio3.code.document") },
            { chiave: "css", etichetta: t("studio3.code.css") },
            { chiave: "differenze", etichetta: t("studio3.code.diff") },
          ]}
        />
        <span className="spinta" />
        {vista === "documento" && (
          <button
            type="button"
            className="pillola btn-ghost minuto"
            disabled={documento === null}
            title={t("studio3.code.format.hint")}
            onClick={() => documento !== null && scriviTesto(stampa(documento), t("studio3.step.format"))}
          >
            {t("studio3.code.format")}
          </button>
        )}
      </div>
      {vista === "documento" && (
        <EditorCodice
          testo={adesso}
          onCambia={(nuovo) => scriviTesto(nuovo, t("studio3.step.code"), "codice")}
          onAnnulla={() => manda({ tipo: "annulla" })}
          onRipeti={() => manda({ tipo: "ripeti" })}
          etichetta={t("studio3.code.document")}
          fonti={fonti}
          segni={segni}
          riga={s.riga}
          onRigaMostrata={(volta) => manda({ tipo: "rigaMostrata", volta })}
          classe="editor-documento"
        />
      )}
      {vista === "css" && <pre className="css-compilato">{s.valido?.css ?? ""}</pre>}
      {vista === "differenze" && <Differenze />}
    </div>
  );
}

/** Che cosa è cambiato dall'apertura: per percorso, o per riga. */
function Differenze() {
  const { s } = useStudio();
  const vai = useVai();
  const [perRiga, setPerRiga] = useState(false);
  const adesso = testo(s);
  const cambi = useMemo(() => {
    const prima = leggi(s.originale);
    const dopo = leggi(adesso);
    return prima === null || dopo === null ? null : differenzePerPercorso(prima, dopo);
  }, [s.originale, adesso]);
  const righe = useMemo(
    () => (perRiga ? differenze(s.originale, adesso) : []),
    [perRiga, s.originale, adesso],
  );
  const conto = quanteCambiano(righe);
  const breve = (v: unknown) =>
    v === undefined ? "∅" : typeof v === "string" ? v : JSON.stringify(v).slice(0, 80);

  return (
    <div className="differenze-dock">
      <label className="spunta">
        <input type="checkbox" checked={perRiga} onChange={(e) => setPerRiga(e.target.checked)} />
        <span>{t("studio3.code.byLine")}</span>
      </label>
      {perRiga ? (
        <>
          <p className="nota-dock">{t("studio3.code.lines", { piu: conto.aggiunte, meno: conto.tolte })}</p>
          <pre className="righe-differenze">
            {righe
              .filter((r) => r.segno !== "=")
              .map((r, i) => (
                <span key={i} data-segno={r.segno === "+" ? "piu" : "meno"}>
                  {r.segno} {r.testo}
                  {"\n"}
                </span>
              ))}
          </pre>
        </>
      ) : cambi === null ? (
        <p className="nota-dock">{t("studio3.code.notJson")}</p>
      ) : cambi.length === 0 ? (
        <p className="nota-dock">{t("studio3.code.noChanges")}</p>
      ) : (
        <ul className="elenco-cambi">
          {cambi.map((c) => (
            <li key={c.percorso}>
              <button type="button" className="voce-uso" onClick={() => vai(c.percorso)}>
                <code>{c.percorso}</code>
                <span className="prima">{breve(c.prima)}</span>
                <Icona nome="i-chev-r" dim={10} />
                <span className="dopo">{breve(c.dopo)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
