/**
 * La timeline: un'animazione come tracce, e i suoi fotogrammi da toccare.
 *
 * # Tracce per proprietà, fotogrammi del formato
 *
 * Il formato scrive un'animazione come una fila di fotogrammi, ognuno con le
 * proprietà che fissa: `{ "at": 40, "scale": 1.24 }`. Un editor di movimento
 * la mostra al contrario, una traccia per proprietà e un rombo dove la
 * proprietà ha un valore — ed è così che si legge «l'opacità arriva a uno a
 * metà, la scala rimbalza dopo». Le due forme sono la stessa informazione: le
 * tracce si ricavano dai fotogrammi, e ogni gesto qui riscrive i fotogrammi.
 *
 * # Suonata dalla Web Animations API
 *
 * La testina si trascina e l'animazione la segue fotogramma per fotogramma,
 * senza aspettare il compilatore: `fotogrammi.ts` traduce il documento con le
 * stesse regole di `compile.rs`. Un rombo trascinato si vede muoversi mentre
 * lo si trascina, e diventa un passo solo della storia.
 */
import { useEffect, useMemo, useRef, useState, type PointerEvent as EventoPuntatore } from "react";

import type { CampoFotogramma } from "../ipc";
import { t, tSe } from "../lingue";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { useAzioniStudio, useStudio } from "./contesto";
import { Cursore, ValoreColore, ValoreLunghezza, ValoreOmbra } from "./controlli";
import { campiAnimati, durataMs, gestoRombo, risolutoriDi, versoAnimazione, type Fotogramma } from "./fotogrammi";
import { parteScelta } from "./negozio";
import { conUnita, latoCambiato, unitaDi } from "./valori";

const oggetto = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/** Il nome di un campo di un fotogramma, per chi legge. */
function nomeCampoFotogramma(nome: string): string {
  return tSe(`studio3.frame.${nome}`, nome);
}

/** Il valore con cui un campo entra in un fotogramma che non l'aveva. */
function valoreNuovo(campo: CampoFotogramma): unknown {
  if (campo.riposo !== null) return campo.riposo;
  switch (campo.kind) {
    case "color":
      return { $token: "color.accent" };
    case "shadow":
      return { $token: "shadow.2" };
    case "clip":
      return { inset: ["0%", "0%", "0%", "0%"] };
    case "length":
      return "0px";
    default:
      return 0;
  }
}

export function Timeline() {
  const { s, manda, registro, documento, scrivi } = useStudio();
  const animazioni = oggetto(oggetto(documento?.["motion"])["animations"]);
  const parte = parteScelta(s);
  const assegnate = useMemo(() => {
    if (parte === null) return [] as [string, string][];
    const movimento = oggetto(oggetto(oggetto(documento?.["parts"])[parte])["motion"]);
    return Object.entries(movimento).filter(
      (voce): voce is [string, string] => typeof voce[1] === "string" && voce[0] !== "stagger",
    );
  }, [parte, documento]);
  const [dallaParte, setDallaParte] = useState<string | null>(null);
  const nome =
    s.selezione?.tipo === "animazione"
      ? s.selezione.nome
      : (assegnate.find(([, n]) => n === dallaParte)?.[1] ?? assegnate[0]?.[1] ?? null);

  if (nome === null || animazioni[nome] === undefined) {
    return (
      <div className="timeline-vuota">
        <Icona nome="i-timeline" dim={22} />
        <p>{t("studio3.timeline.empty")}</p>
        <div className="scelte-timeline">
          {Object.keys(animazioni).map((n) => (
            <button
              key={n}
              type="button"
              className="pillola btn-ghost minuto"
              onClick={() => manda({ tipo: "scegli", selezione: { tipo: "animazione", nome: n } })}
            >
              {n}
            </button>
          ))}
        </div>
      </div>
    );
  }

  return (
    <TimelineDi
      key={nome}
      nome={nome}
      animazione={oggetto(animazioni[nome])}
      assegnate={s.selezione?.tipo === "animazione" ? [] : assegnate}
      onAltra={setDallaParte}
      campiRegistro={registro.fotogramma}
      scriviFotogrammi={(nuovi, etichetta) =>
        scrivi(["motion", "animations", nome, "frames"], nuovi, etichetta)
      }
    />
  );
}

function TimelineDi({
  nome,
  animazione,
  assegnate,
  onAltra,
  campiRegistro,
  scriviFotogrammi,
}: {
  nome: string;
  animazione: Record<string, unknown>;
  /** Le animazioni della parte scelta, per passare dall'una all'altra. */
  assegnate: readonly [string, string][];
  onAltra: (nome: string) => void;
  campiRegistro: readonly CampoFotogramma[];
  scriviFotogrammi: (nuovi: Fotogramma[], etichetta: string) => void;
}) {
  const { s, manda, registro, documento } = useStudio();
  const fotogrammi = useMemo(
    () => (Array.isArray(animazione["frames"]) ? (animazione["frames"] as Fotogramma[]) : []),
    [animazione],
  );
  const [scelto, setScelto] = useState(0);
  const [testina, setTestina] = useState(0);
  const [suona, setSuona] = useState(false);
  const [lento, setLento] = useState(false);
  const palco = useRef<HTMLDivElement>(null);
  const corsa = useRef<Animation | null>(null);
  const righello = useRef<HTMLDivElement>(null);
  const testinaRef = useRef(testina);
  testinaRef.current = testina;
  const durata = Math.max(1, durataMs(animazione["duration"]));
  const campi = campiAnimati(
    fotogrammi,
    campiRegistro.map((c) => c.name),
  );
  const firma = JSON.stringify(animazione);
  const fotogramma = fotogrammi[Math.min(scelto, fotogrammi.length - 1)] ?? { at: 0 };
  const indice = Math.min(scelto, Math.max(0, fotogrammi.length - 1));

  // ── il provino, pilotato dalla testina ──────────────────────────────────
  useEffect(() => {
    const nodo = palco.current;
    if (nodo === null) return;
    const { fotogrammi: chiavi, opzioni } = versoAnimazione(
      animazione,
      risolutoriDi(documento, s.tema === "variante", registro.tokens),
    );
    let presa: Animation | null = null;
    try {
      presa = nodo.animate(chiavi, { ...opzioni, delay: 0, iterations: 1, direction: "normal", fill: "both" });
      presa.pause();
      presa.currentTime = testinaRef.current * durata;
    } catch {
      presa = null;
    }
    corsa.current = presa;
    return () => presa?.cancel();
    // `firma` riassume l'animazione.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firma, documento, s.tema, registro.tokens, durata]);

  useEffect(() => {
    if (!suona && corsa.current !== null) corsa.current.currentTime = testina * durata;
  }, [testina, suona, durata]);

  useEffect(() => {
    if (!suona) return;
    const velocita = lento ? 0.25 : 1;
    const inizio = performance.now() - (testinaRef.current * durata) / velocita;
    let fotogrammaRaf = requestAnimationFrame(function passo(ora) {
      const trascorso = ((ora - inizio) * velocita) % durata;
      if (corsa.current !== null) corsa.current.currentTime = trascorso;
      setTestina(trascorso / durata);
      fotogrammaRaf = requestAnimationFrame(passo);
    });
    return () => cancelAnimationFrame(fotogrammaRaf);
  }, [suona, lento, durata]);

  // ── le scritture ───────────────────────────────────────────────────────
  const cambiaFotogramma = (i: number, nuovo: Fotogramma, etichetta: string) =>
    scriviFotogrammi(
      fotogrammi.map((f, j) => (j === i ? nuovo : f)),
      etichetta,
    );

  const aggiungi = () => {
    if (fotogrammi.length >= registro.maxFotogrammi) return;
    const at = Math.round(testina * 100);
    const gia = fotogrammi.findIndex((f) => Number(f.at) === at);
    if (gia >= 0) {
      setScelto(gia);
      return;
    }
    // Il fotogramma nuovo tiene quel che c'era prima di lui: è una sosta, e da
    // lì si ritocca quel che deve cambiare.
    const dopo = fotogrammi.findIndex((f) => Number(f.at) > at);
    const posto = dopo < 0 ? fotogrammi.length : dopo;
    const prima = fotogrammi[posto - 1] ?? { at: 0, opacity: 1 };
    const nuovo: Fotogramma = { ...prima, at };
    const nuovi = [...fotogrammi.slice(0, posto), nuovo, ...fotogrammi.slice(posto)];
    scriviFotogrammi(nuovi, t("studio3.step.frameAdd", { nome, at }));
    setScelto(posto);
  };

  const togli = (i: number) => {
    if (i === 0 || fotogrammi.length <= registro.minFotogrammi) return;
    scriviFotogrammi(
      fotogrammi.filter((_, j) => j !== i),
      t("studio3.step.frameRemove", { nome, at: Number(fotogrammi[i]?.at ?? 0) }),
    );
    setScelto(Math.max(0, i - 1));
  };

  /** Il posto sul righello sotto il puntatore, da 0 a 1. */
  const posto = (x: number) => {
    const r = righello.current?.getBoundingClientRect();
    if (r === undefined || r.width === 0) return 0;
    return Math.min(1, Math.max(0, (x - r.left) / r.width));
  };

  const scorri = (e: EventoPuntatore<HTMLDivElement>) => {
    setSuona(false);
    setTestina(posto(e.clientX));
    const bersaglio = e.currentTarget;
    bersaglio.setPointerCapture(e.pointerId);
    const muovi = (m: PointerEvent) => setTestina(posto(m.clientX));
    const lascia = () => {
      bersaglio.removeEventListener("pointermove", muovi);
      bersaglio.removeEventListener("pointerup", lascia);
      bersaglio.removeEventListener("pointercancel", lascia);
    };
    bersaglio.addEventListener("pointermove", muovi);
    bersaglio.addEventListener("pointerup", lascia);
    // Un gesto interrotto dal sistema — un tocco che diventa scorrimento, la
    // finestra che perde il puntatore — non manda `pointerup`: senza questo
    // l'ascoltatore restava appeso, e il prossimo passaggio del mouse sul
    // righello spostava la testina senza che nessuno avesse premuto.
    bersaglio.addEventListener("pointercancel", lascia);
  };

  /** Trascina un rombo: sposta `at`, fra il fotogramma prima e quello dopo. */
  const spostaRombo = (e: EventoPuntatore<HTMLButtonElement>, i: number) => {
    setScelto(i);
    setSuona(false);
    if (i === 0) return;
    e.preventDefault();
    const bersaglio = e.currentTarget;
    bersaglio.setPointerCapture(e.pointerId);
    const minimo = Number(fotogrammi[i - 1]?.at ?? 0) + 1;
    const massimo = i === fotogrammi.length - 1 ? 100 : Number(fotogrammi[i + 1]?.at ?? 100) - 1;
    // Con l'ultima posizione scritta, non con quella di partenza: vedi
    // `gestoRombo`.
    const daScrivere = gestoRombo(Number(fotogrammi[i]?.at));
    const muovi = (m: PointerEvent) => {
      const at = Math.min(massimo, Math.max(minimo, Math.round(posto(m.clientX) * 100)));
      const f = fotogrammi[i];
      if (f !== undefined && daScrivere(at)) {
        cambiaFotogramma(i, { ...f, at }, t("studio3.step.frameMove", { nome }));
        setTestina(at / 100);
      }
    };
    const lascia = () => {
      bersaglio.removeEventListener("pointermove", muovi);
      bersaglio.removeEventListener("pointerup", lascia);
      bersaglio.removeEventListener("pointercancel", lascia);
    };
    bersaglio.addEventListener("pointermove", muovi);
    bersaglio.addEventListener("pointerup", lascia);
    bersaglio.addEventListener("pointercancel", lascia);
  };

  const avvisi = (s.esito?.avvisi ?? []).filter((a) => a.path.startsWith(`motion.animations.${nome}`));

  return (
    <div className="timeline-studio">
      <div className="palco-timeline">
        <div className="palco-provino">
          <div ref={palco} className="riquadro-provino" aria-hidden="true">
            <span />
          </div>
        </div>
        <div className="comandi-timeline">
          <button
            type="button"
            className="tasto-icona icon-btn"
            aria-pressed={suona}
            aria-label={suona ? t("studio3.bar.pause") : t("studio3.bar.play")}
            onClick={() => setSuona((x) => !x)}
          >
            <Icona nome={suona ? "i-pause" : "i-play"} dim={14} />
          </button>
          <button
            type="button"
            className="tasto-icona icon-btn"
            aria-pressed={lento}
            title={t("studio3.timeline.slow")}
            aria-label={t("studio3.timeline.slow")}
            onClick={() => setLento((x) => !x)}
          >
            ¼×
          </button>
          <span className="lettura">
            {Math.round(testina * durata)} / {durata}ms
          </span>
        </div>
      </div>

      <div className="tracce-timeline">
        <div className="testa-tracce">
          <code className="nome-animazione">{nome}</code>
          {assegnate.length > 1 && (
            <select
              className="field-input"
              aria-label={t("studio3.timeline.which")}
              value={nome}
              onChange={(e) => onAltra(e.target.value)}
            >
              {assegnate.map(([innesco, n]) => (
                <option key={innesco} value={n}>
                  {innesco} · {n}
                </option>
              ))}
            </select>
          )}
          <span className="spinta" />
          <button
            type="button"
            className="pillola btn-ghost minuto"
            disabled={fotogrammi.length >= registro.maxFotogrammi}
            onClick={aggiungi}
          >
            <Icona nome="i-plus" dim={12} />
            {t("studio3.timeline.addFrame", { at: Math.round(testina * 100) })}
          </button>
          <button
            type="button"
            className="pillola btn-ghost minuto"
            onClick={() => manda({ tipo: "scegli", selezione: { tipo: "animazione", nome } })}
          >
            {t("studio3.timeline.properties")}
          </button>
        </div>
        <div className="griglia-tracce">
          <span className="etichetta-traccia" />
          <div className="righello" ref={righello} onPointerDown={scorri}>
            {[0, 25, 50, 75, 100].map((p) => (
              <span key={p} className="tacca" style={{ left: `${p}%` }}>
                {Math.round((p / 100) * durata)}
              </span>
            ))}
            {fotogrammi.map((f, i) => (
              <button
                key={i}
                type="button"
                className="rombo capo"
                data-scelto={i === indice || undefined}
                style={{ left: `${Number(f.at ?? 0)}%` }}
                aria-label={t("studio3.timeline.frame", { n: i + 1, at: Number(f.at ?? 0) })}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  spostaRombo(e, i);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Delete" || e.key === "Backspace") togli(i);
                }}
              />
            ))}
            <span className="testina" style={{ left: `${testina * 100}%` }} aria-hidden="true" />
          </div>
          {campi.map((campo) => (
            <Traccia
              key={campo}
              campo={campo}
              fotogrammi={fotogrammi}
              scelto={indice}
              testina={testina}
              onScegli={(i) => {
                setScelto(i);
                setSuona(false);
                setTestina(Number(fotogrammi[i]?.at ?? 0) / 100);
              }}
            />
          ))}
        </div>
        {avvisi.map((a, i) => (
          <p key={i} className="nota-dock" data-esito="attenzione">
            <Icona nome="i-alert" dim={12} /> {a.message}
          </p>
        ))}
      </div>

      <EditorFotogramma
        indice={indice}
        fotogramma={fotogramma}
        fotogrammi={fotogrammi}
        campiRegistro={campiRegistro}
        nome={nome}
        onCambia={(nuovo, etichetta) => cambiaFotogramma(indice, nuovo, etichetta)}
        onTogli={() => togli(indice)}
      />
    </div>
  );
}

function Traccia({
  campo,
  fotogrammi,
  scelto,
  testina,
  onScegli,
}: {
  campo: string;
  fotogrammi: readonly Fotogramma[];
  scelto: number;
  testina: number;
  onScegli: (i: number) => void;
}) {
  const presenti = fotogrammi.map((f, i) => [i, f] as const).filter(([, f]) => f[campo] !== undefined);
  return (
    <>
      <span className="etichetta-traccia">{nomeCampoFotogramma(campo)}</span>
      <div className="pista-timeline">
        {presenti.length > 1 && (
          <span
            className="tratto"
            style={{
              left: `${Number(presenti[0]?.[1].at ?? 0)}%`,
              width: `${Number(presenti[presenti.length - 1]?.[1].at ?? 0) - Number(presenti[0]?.[1].at ?? 0)}%`,
            }}
          />
        )}
        {presenti.map(([i, f]) => (
          <button
            key={i}
            type="button"
            className="rombo"
            data-scelto={i === scelto || undefined}
            style={{ left: `${Number(f.at ?? 0)}%` }}
            title={`${nomeCampoFotogramma(campo)} · ${String(Number(f.at ?? 0))}%: ${JSON.stringify(f[campo])}`}
            aria-label={t("studio3.timeline.key", { campo: nomeCampoFotogramma(campo), at: Number(f.at ?? 0) })}
            onClick={() => onScegli(i)}
          />
        ))}
        <span className="testina" style={{ left: `${testina * 100}%` }} aria-hidden="true" />
      </div>
    </>
  );
}

/** I campi di un fotogramma: quali ci sono, e i loro valori. */
function EditorFotogramma({
  indice,
  fotogramma,
  fotogrammi,
  campiRegistro,
  nome,
  onCambia,
  onTogli,
}: {
  indice: number;
  fotogramma: Fotogramma;
  fotogrammi: readonly Fotogramma[];
  campiRegistro: readonly CampoFotogramma[];
  nome: string;
  onCambia: (nuovo: Fotogramma, etichetta: string) => void;
  onTogli: () => void;
}) {
  const { registro, documento } = useStudio();
  const tavolozza = oggetto(documento?.["palette"]) as Record<string, string>;
  const presenti = Object.keys(fotogramma).filter((k) => k !== "at");
  const at = Number(fotogramma.at ?? 0);
  const minimo = indice === 0 ? 0 : Number(fotogrammi[indice - 1]?.at ?? 0) + 1;
  const massimo = indice === 0 ? 0 : indice === fotogrammi.length - 1 ? 100 : Number(fotogrammi[indice + 1]?.at ?? 100) - 1;
  const etichetta = (campo: string) =>
    t("studio3.step.frameField", { nome, at, campo: nomeCampoFotogramma(campo) });
  const scrivi = (campo: string, v: unknown) => {
    const nuovo: Fotogramma = { ...fotogramma };
    if (v === null || v === undefined) delete nuovo[campo];
    else nuovo[campo] = v;
    onCambia(nuovo, etichetta(campo));
  };

  return (
    <div className="editor-fotogramma">
      <div className="testa-fotogramma">
        <strong>{t("studio3.timeline.frame", { n: indice + 1, at })}</strong>
        <button
          type="button"
          className="tasto-icona icon-btn"
          disabled={indice === 0 || fotogrammi.length <= registro.minFotogrammi}
          aria-label={t("studio3.timeline.removeFrame")}
          title={indice === 0 ? t("studio3.timeline.firstStays") : t("studio3.timeline.removeFrame")}
          onClick={onTogli}
        >
          <Icona nome="i-x" dim={12} />
        </button>
      </div>
      <label className="riga-at">
        <span>at</span>
        <input
          type="range"
          min={minimo}
          max={massimo}
          step={1}
          disabled={indice === 0}
          aria-label={t("studio3.timeline.at")}
          value={at}
          onChange={(e) => onCambia({ ...fotogramma, at: Number(e.target.value) }, t("studio3.step.frameMove", { nome }))}
        />
        <span className="lettura">{at}%</span>
      </label>
      <div className="campi-fotogramma">
        {campiRegistro.map((campo) => {
          const acceso = fotogramma[campo.name] !== undefined;
          const ultimo = acceso && presenti.length === 1;
          return (
            <div key={campo.name} className="campo-fotogramma" data-acceso={acceso || undefined}>
              <label className="spunta" title={ultimo ? t("studio3.timeline.lastField") : undefined}>
                <input
                  type="checkbox"
                  checked={acceso}
                  disabled={ultimo}
                  onChange={(e) => scrivi(campo.name, e.target.checked ? valoreNuovo(campo) : null)}
                />
                <span>{nomeCampoFotogramma(campo.name)}</span>
              </label>
              {acceso && (
                <ValoreCampo
                  campo={campo}
                  valore={fotogramma[campo.name]}
                  tavolozza={tavolozza}
                  onCambia={(v) => scrivi(campo.name, v ?? valoreNuovo(campo))}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Il controllo di un campo del fotogramma, secondo il suo tipo nel registro. */
function ValoreCampo({
  campo,
  valore,
  tavolozza,
  onCambia,
}: {
  campo: CampoFotogramma;
  valore: unknown;
  tavolozza: Record<string, string>;
  onCambia: (v: unknown) => void;
}) {
  const { registro } = useAzioniStudio();
  const min = campo.min ?? 0;
  const max = campo.max ?? 1;
  switch (campo.kind) {
    case "number":
      return (
        <Cursore
          valore={valore}
          min={min}
          max={max}
          passo={Number(((max - min) / 100).toFixed(3))}
          etichetta={nomeCampoFotogramma(campo.name)}
          onCambia={onCambia}
        />
      );
    case "angle":
      return (
        <Cursore valore={valore} min={min} max={max} passo={1} suffisso="°" etichetta={nomeCampoFotogramma(campo.name)} onCambia={onCambia} />
      );
    case "offset":
      return <Spostamento valore={valore} campo={campo} onCambia={onCambia} />;
    case "length":
      return (
        <ValoreLunghezza
          valore={valore}
          min={min}
          max={campo.max ?? 40}
          passo={0.5}
          soloAssolute
          etichetta={nomeCampoFotogramma(campo.name)}
          onCambia={onCambia}
        />
      );
    case "color":
      return (
        <ValoreColore
          valore={valore}
          tokens={registro.tokens}
          tavolozza={tavolozza}
          etichetta={nomeCampoFotogramma(campo.name)}
          onCambia={onCambia}
        />
      );
    case "shadow":
      return <ValoreOmbra valore={valore} tokens={registro.tokens} tavolozza={tavolozza} conToken onCambia={onCambia} />;
    case "clip":
      return <Ritaglio valore={valore} onCambia={onCambia} />;
  }
}

/** Uno spostamento: pixel o percentuale, con gli estremi del formato. */
function Spostamento({
  valore,
  campo,
  onCambia,
}: {
  valore: unknown;
  campo: CampoFotogramma;
  onCambia: (v: unknown) => void;
}) {
  const percento = typeof valore === "string" && valore.trim().endsWith("%");
  const numero =
    typeof valore === "number" ? valore : typeof valore === "string" ? Number.parseFloat(valore) || 0 : 0;
  const limite = percento ? 200 : (campo.max ?? 400);
  return (
    <div className="spostamento">
      <Segmentato<"px" | "%">
        etichetta={nomeCampoFotogramma(campo.name)}
        classe="minuto denso"
        scelta={percento ? "%" : "px"}
        onScegli={(u) => onCambia(u === "%" ? `${Math.max(-200, Math.min(200, numero))}%` : numero)}
        voci={[
          { chiave: "px", etichetta: "px" },
          { chiave: "%", etichetta: "%" },
        ]}
      />
      <Cursore
        valore={numero}
        min={-limite}
        max={limite}
        passo={1}
        etichetta={nomeCampoFotogramma(campo.name)}
        onCambia={(v) => onCambia(v === null ? null : percento ? `${String(v)}%` : v)}
      />
    </div>
  );
}

/** Un ritaglio: nessuno, un rettangolo rientrato, un cerchio. */
function Ritaglio({ valore, onCambia }: { valore: unknown; onCambia: (v: unknown) => void }) {
  const d = oggetto(valore);
  const forma = Array.isArray(d["inset"]) ? "inset" : d["circle"] !== undefined ? "circle" : "polygon";
  const scritti: unknown[] = Array.isArray(d["inset"]) ? (d["inset"] as unknown[]) : [];
  const lati = Array.isArray(d["inset"]) ? scritti.map((x) => Number.parseFloat(String(x)) || 0) : [0, 0, 0, 0];
  const raggio = Number.parseFloat(String(oggetto(d["circle"])["radius"] ?? "50")) || 0;
  return (
    <div className="ritaglio-fotogramma">
      <Segmentato<"inset" | "circle" | "polygon">
        etichetta={nomeCampoFotogramma("clip")}
        classe="minuto denso"
        scelta={forma}
        onScegli={(f) =>
          onCambia(
            f === "inset"
              ? { inset: ["0%", "0%", "0%", "0%"] }
              : f === "circle"
                ? { circle: { radius: "75%" } }
                : { polygon: [["50%", "0%"], ["100%", "100%"], ["0%", "100%"]] },
          )
        }
        voci={[
          { chiave: "inset", etichetta: "inset" },
          { chiave: "circle", etichetta: "circle" },
          { chiave: "polygon", etichetta: "polygon" },
        ]}
      />
      {forma === "inset" &&
        (["top", "right", "bottom", "left"] as const).map((lato, i) => (
          <Cursore
            key={lato}
            valore={lati[i] ?? 0}
            min={0}
            max={50}
            passo={1}
            suffisso={unitaDi(scritti[i]) ?? "%"}
            etichetta={tSe(`studio3.clip.${lato}`, lato)}
            // Gli altri lati restano come sono scritti, e questo tiene la sua
            // unità: vedi `latoCambiato`.
            onCambia={(v) => onCambia({ inset: latoCambiato(scritti, i, Number(v ?? 0)) })}
          />
        ))}
      {forma === "circle" && (
        <Cursore
          valore={raggio}
          min={0}
          max={150}
          passo={1}
          suffisso={unitaDi(oggetto(d["circle"])["radius"]) ?? "%"}
          etichetta={t("studio3.clip.radius")}
          onCambia={(v) =>
            onCambia({
              circle: { ...oggetto(d["circle"]), radius: conUnita(oggetto(d["circle"])["radius"], Number(v ?? 0), "%") },
            })
          }
        />
      )}
      {forma === "polygon" && <p className="nota-ispezione">{t("studio3.clip.polygonHint")}</p>}
    </div>
  );
}
